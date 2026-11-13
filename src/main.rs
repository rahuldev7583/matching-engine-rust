use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    env,
    fmt::Debug,
    str::FromStr,
    thread::sleep,
    time::Duration,
};

mod publisher;

use anyhow::{Error, Result};
use chrono::Utc;
use dotenvy::dotenv;
use publisher::Publisher;
use redis::{Commands, Connection, JsonCommands, RedisError, cmd};
use serde::{Deserialize, Serialize, de::value::Error as SerdeError};
use serde_json::Error as SerdeJsonError;
use serde_json::from_str;
use strum::EnumString;
use uuid::Uuid;

use crate::{
    EngineCommand::{CancelOrder, CreateOrder, ForcedLiquidate, GetBalance, GetDepth, GetOrder},
    EngineStatus::{Down, Pending, Ready},
    OrderRequestType::Perp,
};

use matching_engine_rust::{
    order::{Depth, Orderbook},
    orderbook::{
        Fill, ORDERBOOK, OpenOrder,
        OrderStatus::{self, Accepted, Cancelled, NotFound, Rejected},
        OrderType::{self, Limit},
        Side::{self, Buy, Sell},
    },
};

//NOTE:
//  1. get the availabe markets from backend and initialize order book for each market, for now => start with demo
//  2. add market check before processing order
//  3. for backend incoming request
//          => para for create order = backend_id, request_id and payload (order)
//          => para for cancel/get order => backend_id, request_id and payload (order_id)
//          => para for get depth => backend_id, request_id, and payload(symbol)
//  4. for liquidation request
//          => para for forced-liquidate = backend_id, request_id, payload (order)
//  5. for risk engine request
//          => para for create order = backend_id, request_id, payload(order) and request type =>"perp"
//          => para for close position = backend_id, request_id, payload(order), request type => "perp" and command => closedposition
//

//NOTE:
//create order =>
//      ii. if full/paritaily filled =>
//                 =>  db_worker_client with =>{ fill, payload, request_type: request_type, order_id, command: command || 'create-fill' }
//                       = request_type only for "perp"
//                       = command only for close-position
//               => riskengpubclient with => {fill, command: "fill-order"}
//      iii. after looping all orders =>  publishclient with => engineresponse to queue with
//      backend_id
//cancel order =>
//      i. publishclient with => engineresponse with the backend_id queue
//      ii. riskengpubclient => {order: order, command: "cancel-order"}
//      iii. dbworkerclient => { request_id, filled_quantity: 0, message: '', ...order, fill: {}, payload: { symbol: order.symbol }, command: "cancel-order" }
//get order =>
//         publishclient =>  request_id,         data: order,         status: 'order pending',         message: 'order is sitting on orderbook',         order_id: '',         price: 0,         filled_quantity: 0
//get depth =>
//          publishclient => request_id,         symbol: payload,         bids: bids,         asks: asks,         timestamp: date.now()

#[derive(PartialEq)]
pub enum EngineStatus {
    Ready,
    Pending,
    Down,
}

impl EngineStatus {
    fn new() -> Self {
        Self::Down
    }
}

pub struct RedisServics {
    pub client: Connection,
    pub publish_client: Connection,
    pub db_worker_client: Connection,
    pub risk_engine_pub_client: Connection,
}

const INCOMING_QUEUE: &str = "incoming-request";
const LIQUIDATION_QUEUE: &str = "liquidation-to-matching-eng";
const RISK_QUEUE: &str = "risk-to-matching-eng";

fn new_redis_client(url: String) -> Result<Connection, Error> {
    match redis::Client::open(url.clone()) {
        Ok(client) => match client.get_connection() {
            Ok(conn) => Ok(conn),
            Err(e) => {
                println!("Failed to connect to Redis: {e}");
                Err(Error::from(e))
            }
        },
        Err(e) => {
            println!("Failed to create Redis client: {e}");
            Err(Error::from(e))
        }
    }
}
impl RedisServics {
    pub fn new(url: String) -> Self {
        // println!("url: {}", url);

        let mut client = new_redis_client(url.clone()).unwrap();

        let status: String = client.ping().unwrap();

        // println!("status: {}", status);

        Self {
            client: new_redis_client(url.clone()).unwrap(),
            publish_client: new_redis_client(url.clone()).unwrap(),
            db_worker_client: new_redis_client(url.clone()).unwrap(),
            risk_engine_pub_client: new_redis_client(url.clone()).unwrap(),
        }
    }
}

#[derive(Deserialize, Debug, PartialEq, Serialize, EnumString)]
pub enum EngineCommand {
    #[strum(serialize = "create-order")]
    CreateOrder,
    #[strum(serialize = "cancel-order")]
    CancelOrder,
    #[strum(serialize = "get-order")]
    GetOrder,
    #[strum(serialize = "get-depth")]
    GetDepth,
    #[strum(serialize = "get-balance")]
    GetBalance,

    #[strum(serialize = "forced-liquidate")]
    ForcedLiquidate,
    #[strum(serialize = "close-position")]
    ClosePosition,
}

#[derive(Deserialize, Debug, PartialEq, Serialize, Clone)]
pub struct OrderPayload {
    r#type: String,
    quantity: String,
    price: Option<String>,
    symbol: String,
    user_id: u64,
    side: String, // command: Option<String>,
}

#[derive(Deserialize, Debug, PartialEq, Serialize, Clone)]
pub struct GetCancelOrderPayload {
    symbol: String,
    order_id: String,
}

#[derive(Deserialize, Debug, PartialEq, Serialize, Clone)]
#[serde(untagged)]
pub enum Payload {
    Order(OrderPayload),
    GetOrCancelOrder(GetCancelOrderPayload),
    GetDepth(String),
}

//TODO: engine request payload
#[derive(Deserialize, Debug, Serialize)]
pub struct EngineRequest {
    pub command: String,
    pub request_id: String,
    pub payload: Payload,
    pub BACKEND_ID: String,
}

#[derive(Debug, EnumString, Serialize, Deserialize, PartialEq, Eq, Clone, Copy)]
pub enum OrderRequestType {
    #[strum(serialize = "spot")]
    Spot,
    #[strum(serialize = "perp")]
    Perp,
}

#[derive(Deserialize, Debug, PartialEq, Serialize, Clone)]
pub struct EngineResponse {
    pub request_id: String,
    pub order_id: String,
    pub filled_quantity: u64,
    pub remaining_quantity: u64,
    pub status: OrderStatus,
    pub price: u64,
    pub message: String,

    pub data: Option<OpenOrder>,
    pub fills: Vec<Fill>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct DBPayloadRes {
    symbol: String,
}

#[derive(Deserialize, Debug, PartialEq, Serialize, Clone)]
#[serde(untagged)]
pub enum DBPayload {
    Engine(EngineResponse),
    CancelOrder(DBPayloadRes),
}

#[derive(Deserialize, Debug, PartialEq, Serialize, Clone)]
pub struct DBWorkerResponse {
    request_id: Option<String>,
    fill: Option<Fill>,
    order: Option<OpenOrder>,
    payload: DBPayload,
    request_type: Option<OrderRequestType>,
    order_id: String,
    //either command or 'create-fill'
    command: String,
}

#[derive(Deserialize, Debug, PartialEq, Serialize, Clone)]
pub struct RiskEngineResponse {
    fill: Option<Fill>,
    order: Option<OpenOrder>,
    command: String,
}

#[derive(Deserialize, Debug, PartialEq, Serialize, Clone)]
pub struct DepthResponse {
    request_id: String,
    symbol: String,
    asks: Vec<Depth>,
    bids: Vec<Depth>,
    timestamp: i64,
}

#[derive(Deserialize, Debug, PartialEq, Serialize, Clone)]
pub struct Market {
    id: u64,
    name: String,
    symbol: String,
    base_asset_id: u64,
    quote_asset_id: u64,
    created_at: String,
    updated_at: String,
    r#type: String,
    adminAdmin_id: u64,
}

#[derive(Deserialize, Debug, PartialEq, Serialize, Clone)]
pub struct MarketResponse {
    message: String,
    markets: Vec<Market>,
}

// Fixture format (ENGINE_CONTRACT.md §5): {"markets":[{"symbol":"BTC_USDC"},...]}
#[derive(Deserialize)]
struct Fixture {
    markets: Vec<FixtureMarket>,
}

#[derive(Deserialize)]
struct FixtureMarket {
    symbol: String,
}

// Load market symbols from MARKETS_FIXTURE (inline JSON or a file path).
fn load_fixture_symbols() -> Option<Vec<String>> {
    let raw = env::var("MARKETS_FIXTURE").ok()?;
    let trimmed = raw.trim();
    let text = if trimmed.starts_with('{') {
        trimmed.to_string()
    } else {
        std::fs::read_to_string(trimmed).ok()?
    };
    let fixture: Fixture = serde_json::from_str(&text).ok()?;
    Some(fixture.markets.into_iter().map(|m| m.symbol).collect())
}

fn main() {
    let mut engine_status = EngineStatus::new();

    dotenv().ok();

    let verbose = env::var("VERBOSE_LOGGING")
        .map(|v| v == "true")
        .unwrap_or(false);

    let mut orderbooks: HashMap<String, ORDERBOOK> = HashMap::new();

    if let Some(symbols) = load_fixture_symbols() {
        for symbol in symbols {
            orderbooks.insert(symbol, ORDERBOOK::new());
        }
    } else {
        let BACKEND_URL = env::var("BACKEND_URL").expect("BACKEND_URL is not defined");
        let body = reqwest::blocking::get(BACKEND_URL);

        let text = body.unwrap().text().unwrap();

        let parsed_markets: Result<MarketResponse, SerdeJsonError> = serde_json::from_str(&text);

        if parsed_markets.is_ok() {
            let all_markets = parsed_markets.unwrap();
            for m in all_markets.markets {
                orderbooks.insert(m.symbol, ORDERBOOK::new());
            }
        }
    }

    let REDIS_URL = env::var("REDIS_URL").expect("REDIS_URL is not defined");

    let RedisServics {
        mut client,
        publish_client,
        db_worker_client,
        risk_engine_pub_client,
    } = RedisServics::new(REDIS_URL.clone());

    // Writes never block the matching thread (ENGINE_CONTRACT.md §4).
    let publisher = Publisher::new(
        &REDIS_URL,
        publish_client,
        db_worker_client,
        risk_engine_pub_client,
        verbose,
    );

    loop {
        // Timeout 0 blocks until a request arrives
        let request: Result<(String, String), RedisError> = cmd("BRPOP")
            .arg(LIQUIDATION_QUEUE)
            .arg(INCOMING_QUEUE)
            .arg(RISK_QUEUE)
            .arg(0)
            .query(&mut client);

        match request {
            Ok((key, value)) => {
                let parsed_val: Result<EngineRequest, SerdeJsonError> =
                    serde_json::from_str(&value);

                match parsed_val {
                    Ok(eng_req) => match key.as_str() {
                        INCOMING_QUEUE => process_incoming(eng_req, &mut orderbooks, &publisher),
                        LIQUIDATION_QUEUE => {
                            process_liquidation(eng_req, &mut orderbooks, &publisher)
                        }
                        RISK_QUEUE => process_risk(eng_req, &mut orderbooks, &publisher),
                        _ => {
                            if verbose {
                                println!("unexpected queue: {key}");
                            }
                        }
                    },
                    Err(e) => {
                        if verbose {
                            println!("failed to parse request: {e}");
                        }
                    }
                }
            }
            Err(e) => {
                if verbose {
                    println!("RedisError: {e}");
                }
            }
        }
    }
}

fn response_queue(backend_id: &str) -> String {
    format!("response-queue-{backend_id}")
}

fn publish_response(publisher: &Publisher, backend_id: &str, res: &EngineResponse) {
    let json = serde_json::to_string(res).expect("Failed to convert JSON");
    publisher.response(response_queue(backend_id), json);
}

fn publish_settlement(publisher: &Publisher, msg: &DBWorkerResponse) {
    publisher.settlement(serde_json::to_string(msg).expect("Failed to convert to json"));
}

fn publish_risk(publisher: &Publisher, msg: &RiskEngineResponse) {
    publisher.risk(serde_json::to_string(msg).expect("Failed to convert json"));
}

fn empty_response(
    request_id: &str,
    order_id: &str,
    status: OrderStatus,
    message: &str,
) -> EngineResponse {
    EngineResponse {
        request_id: request_id.to_string(),
        order_id: order_id.to_string(),
        filled_quantity: 0,
        remaining_quantity: 0,
        status,
        price: 0,
        message: message.to_string(),
        data: None,
        fills: vec![],
    }
}

// Create an order and publish response + side effects.
//   request_type: Some(Perp) for liquidation/risk orders, None for spot
//   fill_command: command recorded on settlement messages for fills
fn handle_create_order(
    eng_req: &EngineRequest,
    orderbooks: &mut HashMap<String, ORDERBOOK>,
    publisher: &Publisher,
    request_type: Option<OrderRequestType>,
    fill_command: &str,
) {
    let backend_id = &eng_req.BACKEND_ID;
    let request_id = &eng_req.request_id;

    let payload = match &eng_req.payload {
        Payload::Order(p) => p.clone(),
        _ => return,
    };

    let order_id = Uuid::new_v4().to_string();
    let quantity: u64 = match payload.quantity.trim().parse() {
        Ok(q) => q,
        Err(_) => {
            let res = empty_response(request_id, &order_id, Rejected, "Invalid quantity");
            publish_response(publisher, backend_id, &res);
            return;
        }
    };
    let price: u64 = payload
        .price
        .as_deref()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let (order_type, side) = match (
        OrderType::from_str(&payload.r#type),
        Side::from_str(&payload.side),
    ) {
        (Ok(t), Ok(s)) => (t, s),
        _ => {
            let res = EngineResponse {
                remaining_quantity: quantity,
                price,
                ..empty_response(
                    request_id,
                    &order_id,
                    Rejected,
                    "Invalid order type or side",
                )
            };
            publish_response(publisher, backend_id, &res);
            return;
        }
    };

    let open_order = OpenOrder {
        backend_id: backend_id.clone(),
        request_id: request_id.clone(),
        order_id: order_id.clone(),
        user_id: payload.user_id.to_string(),
        order_type,
        side,
        symbol: payload.symbol,
        quantity,
        price,
        created_at: Utc::now().timestamp_millis(),
    };

    let orderbook = match orderbooks.get_mut(&open_order.symbol) {
        Some(ob) => ob,
        None => {
            let res = EngineResponse {
                remaining_quantity: quantity,
                price,
                ..empty_response(
                    request_id,
                    &order_id,
                    Rejected,
                    "Market not available on orderbook",
                )
            };
            publish_response(publisher, backend_id, &res);
            return;
        }
    };

    match ORDERBOOK::create_order(orderbook, open_order.clone()) {
        Ok(order_detail) => {
            let is_market = open_order.order_type == OrderType::Market;

            // Market orders report the average execution price (integer division),
            // limit orders their limit price.
            let response_price = if is_market && order_detail.filled_quantity > 0 {
                let notional: u128 = order_detail
                    .fills
                    .iter()
                    .map(|f| f.price as u128 * f.filled_quantity as u128)
                    .sum();
                (notional / order_detail.filled_quantity as u128) as u64
            } else {
                open_order.price
            };

            let payload_res_data = EngineResponse {
                request_id: request_id.clone(),
                order_id: open_order.order_id.clone(),
                filled_quantity: order_detail.filled_quantity,
                remaining_quantity: open_order.quantity - order_detail.filled_quantity,
                status: order_detail.status.clone(),
                price: response_price,
                message: "Order created successfully".to_string(),
                data: None,
                fills: order_detail.fills.clone(),
            };

            for current_fill in &order_detail.fills {
                publish_settlement(
                    publisher,
                    &DBWorkerResponse {
                        request_id: Some(request_id.clone()),
                        fill: Some(current_fill.clone()),
                        payload: DBPayload::Engine(payload_res_data.clone()),
                        request_type,
                        order_id: order_detail.order_id.clone(),
                        command: fill_command.to_string(),
                        order: None,
                    },
                );
                publish_risk(
                    publisher,
                    &RiskEngineResponse {
                        fill: Some(current_fill.clone()),
                        command: "fill-order".to_string(),
                        order: None,
                    },
                );
            }

            // A limit order with remaining quantity rests on the book.
            let rested = !is_market
                && order_detail.filled_quantity < open_order.quantity
                && matches!(order_detail.status, Accepted | OrderStatus::PartiallyFilled);
            if rested {
                let resting = OpenOrder {
                    quantity: open_order.quantity - order_detail.filled_quantity,
                    ..open_order.clone()
                };
                publish_risk(
                    publisher,
                    &RiskEngineResponse {
                        fill: None,
                        order: Some(resting.clone()),
                        command: "create-order".to_string(),
                    },
                );
                publish_settlement(
                    publisher,
                    &DBWorkerResponse {
                        request_id: Some(request_id.clone()),
                        fill: None,
                        payload: DBPayload::Engine(payload_res_data.clone()),
                        request_type,
                        order_id: order_detail.order_id.clone(),
                        command: "create-order".to_string(),
                        order: Some(resting),
                    },
                );
            }

            publish_response(publisher, backend_id, &payload_res_data);
        }
        Err(e) => {
            let res = EngineResponse {
                remaining_quantity: open_order.quantity,
                price: open_order.price,
                ..empty_response(request_id, &order_id, Rejected, &e.to_string())
            };
            publish_response(publisher, backend_id, &res);
        }
    }
}

// cancel-order / get-order payloads: either a bare order id (what the backend and the
// TypeScript engine use) or {symbol, order_id}.
fn order_id_and_symbol(payload: &Payload) -> Option<(String, Option<String>)> {
    match payload {
        Payload::GetDepth(order_id) => Some((order_id.clone(), None)),
        Payload::GetOrCancelOrder(p) => Some((p.order_id.clone(), Some(p.symbol.clone()))),
        Payload::Order(_) => None,
    }
}

fn find_symbol(
    orderbooks: &HashMap<String, ORDERBOOK>,
    order_id: &str,
    hint: Option<String>,
) -> Option<String> {
    if let Some(symbol) = hint {
        if orderbooks
            .get(&symbol)
            .map_or(false, |ob| ob.orders.contains_key(order_id))
        {
            return Some(symbol);
        }
    }
    orderbooks
        .iter()
        .find(|(_, ob)| ob.orders.contains_key(order_id))
        .map(|(symbol, _)| symbol.clone())
}

fn handle_cancel_order(
    eng_req: &EngineRequest,
    orderbooks: &mut HashMap<String, ORDERBOOK>,
    publisher: &Publisher,
) {
    let backend_id = &eng_req.BACKEND_ID;
    let request_id = &eng_req.request_id;
    let Some((order_id, hint)) = order_id_and_symbol(&eng_req.payload) else {
        return;
    };

    let cancelled = find_symbol(orderbooks, &order_id, hint)
        .and_then(|symbol| orderbooks.get_mut(&symbol))
        .and_then(|ob| ORDERBOOK::cancel_order(ob, order_id.clone()).ok());

    match cancelled {
        Some(order) => {
            let res = EngineResponse {
                request_id: request_id.clone(),
                order_id: order.order_id.clone(),
                filled_quantity: 0,
                remaining_quantity: order.quantity,
                status: Cancelled,
                price: order.price,
                message: "Order cancelled".to_string(),
                data: Some(order.clone()),
                fills: vec![],
            };

            publish_settlement(
                publisher,
                &DBWorkerResponse {
                    request_id: Some(request_id.clone()),
                    fill: None,
                    payload: DBPayload::CancelOrder(DBPayloadRes {
                        symbol: order.symbol.clone(),
                    }),
                    request_type: None,
                    order: Some(order.clone()),
                    order_id: order.order_id.clone(),
                    command: "cancel-order".to_string(),
                },
            );
            publish_risk(
                publisher,
                &RiskEngineResponse {
                    order: Some(order),
                    command: "cancel-order".to_string(),
                    fill: None,
                },
            );
            publish_response(publisher, backend_id, &res);
        }
        None => {
            let res = empty_response(request_id, "", NotFound, "Order not found");
            publish_response(publisher, backend_id, &res);
        }
    }
}

fn handle_get_order(
    eng_req: &EngineRequest,
    orderbooks: &mut HashMap<String, ORDERBOOK>,
    publisher: &Publisher,
) {
    let backend_id = &eng_req.BACKEND_ID;
    let request_id = &eng_req.request_id;
    let Some((order_id, hint)) = order_id_and_symbol(&eng_req.payload) else {
        return;
    };

    let found = find_symbol(orderbooks, &order_id, hint)
        .and_then(|symbol| orderbooks.get(&symbol))
        .and_then(|ob| ORDERBOOK::get_order(ob, order_id.clone()).ok());

    let res = match found {
        Some(o) => EngineResponse {
            request_id: request_id.clone(),
            order_id: o.order_id.clone(),
            filled_quantity: 0,
            remaining_quantity: o.quantity,
            status: Accepted,
            price: o.price,
            message: "Order sitting on book".to_string(),
            data: Some(o),
            fills: vec![],
        },
        None => empty_response(request_id, "", NotFound, "Order not found"),
    };
    publish_response(publisher, backend_id, &res);
}

fn handle_get_depth(
    eng_req: &EngineRequest,
    orderbooks: &mut HashMap<String, ORDERBOOK>,
    publisher: &Publisher,
) {
    let Payload::GetDepth(symbol) = &eng_req.payload else {
        return;
    };

    // Unknown market: empty depth, like the TypeScript engine. No level cap.
    let depth = match orderbooks.get(symbol) {
        Some(ob) => ORDERBOOK::get_depth(ob, u64::MAX),
        None => matching_engine_rust::order::DepthDetail {
            asks: vec![],
            bids: vec![],
            timestamp: Utc::now().timestamp_millis(),
        },
    };

    let res_data = DepthResponse {
        request_id: eng_req.request_id.clone(),
        symbol: symbol.clone(),
        asks: depth.asks,
        bids: depth.bids,
        timestamp: depth.timestamp,
    };
    let res_json = serde_json::to_string(&res_data).expect("Failed to convert JSON");
    publisher.response(response_queue(&eng_req.BACKEND_ID), res_json);
}

// Requests are matched on the command string; unknown or unsupported commands are ignored
// (the TypeScript engine does the same) instead of panicking the engine.
fn process_incoming(
    eng_req: EngineRequest,
    orderbooks: &mut HashMap<String, ORDERBOOK>,
    publisher: &Publisher,
) {
    let Ok(command) = EngineCommand::from_str(&eng_req.command) else {
        return;
    };
    match command {
        CreateOrder => handle_create_order(&eng_req, orderbooks, publisher, None, "create-fill"),
        CancelOrder => handle_cancel_order(&eng_req, orderbooks, publisher),
        GetOrder => handle_get_order(&eng_req, orderbooks, publisher),
        GetDepth => handle_get_depth(&eng_req, orderbooks, publisher),
        GetBalance | ForcedLiquidate | EngineCommand::ClosePosition => {}
    }
}

fn process_liquidation(
    eng_req: EngineRequest,
    orderbooks: &mut HashMap<String, ORDERBOOK>,
    publisher: &Publisher,
) {
    if let Ok(ForcedLiquidate) = EngineCommand::from_str(&eng_req.command) {
        handle_create_order(&eng_req, orderbooks, publisher, Some(Perp), "create-fill");
    }
}

fn process_risk(
    eng_req: EngineRequest,
    orderbooks: &mut HashMap<String, ORDERBOOK>,
    publisher: &Publisher,
) {
    match EngineCommand::from_str(&eng_req.command) {
        Ok(CreateOrder) => {
            handle_create_order(&eng_req, orderbooks, publisher, Some(Perp), "create-fill")
        }
        Ok(EngineCommand::ClosePosition) => handle_create_order(
            &eng_req,
            orderbooks,
            publisher,
            Some(Perp),
            "close-position",
        ),
        _ => {}
    }
}
