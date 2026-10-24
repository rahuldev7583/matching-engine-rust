use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    env,
    fmt::Debug,
    str::FromStr,
    thread::sleep,
    time::Duration,
};

use anyhow::{Error, Result};
use chrono::Utc;
use dotenvy::dotenv;
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
    price: String,
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
    pub status: OrderStatus,
    pub price: u64,
    pub message: String,

    pub data: Option<OpenOrder>,
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

fn main() {
    // println!("Hello, world!");
    let mut engine_status = EngineStatus::new();

    dotenv().ok();

    let BACKEND_URL = env::var("BACKEND_URL").expect("BACKEND_URL is not defined");
    let body = reqwest::blocking::get(BACKEND_URL);

    // println!("{:?}", body);

    let mut orderbooks: HashMap<String, ORDERBOOK> = HashMap::new();

    let text = body.unwrap().text().unwrap();

    // println!("text: {}", text);

    let parsed_markets: Result<MarketResponse, SerdeJsonError> = serde_json::from_str(&text);

    // println!("PARSED_MARKETS: {:?}", parsed_markets);

    if parsed_markets.is_ok() {
        let all_markets = parsed_markets.unwrap();
        // println!("MARKETS: {:?}", all_markets);

        for m in all_markets.markets {
            orderbooks.insert(m.symbol, ORDERBOOK::new());
        }
    }

    // println!("orderbooks: {:?}", orderbooks);

    let REDIS_URL = env::var("REDIS_URL").expect("REDIS_URL is not defined");

    // println!("REDIS_URL:{}", &REDIS_URL);

    let redis_clients = RedisServics::new(REDIS_URL.clone());

    // println!("redis services");

    let RedisServics {
        mut client,
        mut publish_client,
        mut db_worker_client,
        mut risk_engine_pub_client,
    } = redis_clients;

    // println!("ALL REDIS SERVICES CONNECTED");

    loop {
        let request: Result<(String, String), RedisError> = cmd("BRPOP")
            .arg(INCOMING_QUEUE)
            .arg(LIQUIDATION_QUEUE)
            .arg(RISK_QUEUE)
            .arg(0.1)
            .query(&mut client);

        match request {
            Ok((key, value)) => {
                let parsed_val: Result<EngineRequest, SerdeJsonError> =
                    serde_json::from_str(&value);

                match parsed_val {
                    Ok(eng_req) => match key.as_str() {
                        INCOMING_QUEUE => process_incoming(
                            eng_req,
                            &mut orderbooks,
                            &mut publish_client,
                            &mut db_worker_client,
                            &mut risk_engine_pub_client,
                        ),
                        LIQUIDATION_QUEUE => process_liquidation(
                            eng_req,
                            &mut orderbooks,
                            &mut publish_client,
                            &mut db_worker_client,
                            &mut risk_engine_pub_client,
                        ),
                        RISK_QUEUE => process_risk(
                            eng_req,
                            &mut orderbooks,
                            &mut publish_client,
                            &mut db_worker_client,
                            &mut risk_engine_pub_client,
                        ),
                        _ => println!("unexpected queue: {key}"),
                    },
                    Err(e) => println!("failed to parse request: {e}"),
                }
            }
            Err(e) => println!("RedisError: {e}"),
        }
    }
}

fn process_incoming(
    eng_req: EngineRequest,
    orderbooks: &mut HashMap<String, ORDERBOOK>,
    publish_client: &mut Connection,
    db_worker_client: &mut Connection,
    risk_engine_pub_client: &mut Connection,
) {
    match EngineCommand::from_str(&eng_req.command).unwrap() {
        CreateOrder => {
            let backend_id = eng_req.BACKEND_ID.clone();
            let request_id = eng_req.request_id.clone();

            let payload = match eng_req.payload.clone() {
                Payload::Order(order_payload) => Ok(order_payload),
                Payload::GetOrCancelOrder(_) => Err(Error::msg("not valid payload")),
                Payload::GetDepth(_) => Err(Error::msg("not valid payload")),
            };

            if payload.is_ok() {
                let payload = payload.unwrap();
                let (price, quantity): (u64, u64) = (
                    payload.price.trim().parse().unwrap(),
                    payload.quantity.parse().unwrap(),
                );

                let open_order = OpenOrder {
                    backend_id: backend_id.clone(),
                    request_id: request_id.clone(),
                    order_id: Uuid::new_v4().to_string(),
                    user_id: payload.user_id.to_string(),
                    order_type: OrderType::from_str(&payload.r#type).unwrap(),
                    side: Side::from_str(&payload.side).unwrap(),
                    symbol: payload.symbol,
                    quantity,
                    price,
                    created_at: Utc::now().timestamp_millis(),
                };
                let mut orderbook = orderbooks.get_mut(&open_order.symbol).unwrap();
                let ord = ORDERBOOK::create_order(&mut orderbook, open_order.clone());

                match ord {
                    Ok(order_detail) => {
                        let payload_res_data = EngineResponse {
                            request_id: request_id.clone(),
                            order_id: open_order.order_id,
                            filled_quantity: order_detail.filled_quantity,
                            status: order_detail.status,
                            price: open_order.price,
                            message: "Order created successfully".to_string(),
                            data: None,
                        };
                        let fill = order_detail.fill;

                        if fill.is_some() {
                            let current_fill = fill.unwrap();
                            let payload_res = DBPayload::Engine(payload_res_data.clone());

                            let db_res_data = DBWorkerResponse {
                                request_id: Some(request_id.clone()),
                                fill: Some(current_fill.clone()),
                                payload: payload_res,
                                request_type: None,
                                order_id: order_detail.order_id,
                                command: "create-fill".to_string(),
                                order: None,
                            };

                            let db_res_json = serde_json::to_string(&db_res_data)
                                .expect("Failed to convert to json");

                            db_worker_client.lpush::<_, _, ()>("settlement-queue", db_res_json);

                            let risk_res_data = RiskEngineResponse {
                                fill: Some(current_fill),
                                command: "fill-order".to_string(),
                                order: None,
                            };

                            let risk_res_json = serde_json::to_string(&risk_res_data)
                                .expect("Failed to convert json");

                            risk_engine_pub_client
                                .lpush::<_, _, ()>("matching-to-risk-pub-queue", risk_res_json);
                        }

                        let res_json: String = serde_json::to_string(&payload_res_data)
                            .expect("Failed to convert JSON");

                        let res_queue: String = format!("response-queue-{backend_id}-{request_id}");

                        publish_client.lpush::<_, _, ()>(res_queue, res_json);
                    }
                    Err(e) => {
                        let res_data = EngineResponse {
                            request_id: request_id.clone(),
                            order_id: open_order.order_id,
                            filled_quantity: 0,
                            status: Rejected,
                            price: open_order.price,
                            message: e.to_string(),
                            data: None,
                        };

                        let res_json: String =
                            serde_json::to_string(&res_data).expect("Failed to convert JSON");

                        let res_queue: String = format!("response-queue-{backend_id}-{request_id}");

                        publish_client.lpush::<_, _, ()>(res_queue, res_json);
                    }
                }
            }
        }
        EngineCommand::CancelOrder => {
            let backend_id = eng_req.BACKEND_ID.clone();
            let request_id = eng_req.request_id.clone();

            let payload = match eng_req.payload.clone() {
                Payload::Order(order_payload) => Err(Error::msg("not valid payload")),
                Payload::GetOrCancelOrder(order) => Ok(order),
                Payload::GetDepth(_) => Err(Error::msg("not valid payload")),
            };

            if let Ok(orderPayload) = &payload {
                let order_id = orderPayload.to_owned().order_id;
                let symbol = orderPayload.to_owned().symbol;

                let mut orderbook = orderbooks.get_mut(&symbol).unwrap();
                let ord = ORDERBOOK::cancel_order(&mut orderbook, order_id.clone());

                match ord {
                    Ok(ord) => {
                        let res_data = EngineResponse {
                            request_id: request_id.clone(),
                            order_id: order_id.to_owned(),
                            filled_quantity: 0,
                            status: Cancelled,
                            price: 0,
                            message: "Order cancelled".to_string(),
                            data: None,
                        };

                        let res_json: String =
                            serde_json::to_string(&res_data).expect("Failed to convert JSON");

                        let res_queue: String = format!("response-queue-{backend_id}-{request_id}");

                        publish_client.lpush::<_, _, ()>(res_queue, res_json);

                        let current_order = ord;
                        let payload_res = DBPayload::CancelOrder(DBPayloadRes {
                            symbol: current_order.symbol.clone(),
                        });

                        let db_res_data = DBWorkerResponse {
                            request_id: Some(request_id.clone()),
                            fill: None,
                            payload: payload_res,
                            request_type: None,
                            order: Some(current_order.clone()),
                            order_id: current_order.clone().order_id,
                            command: "cancel-order".to_string(),
                        };

                        let db_res_json =
                            serde_json::to_string(&db_res_data).expect("Failed to convert to json");

                        db_worker_client.lpush::<_, _, ()>("settlement-queue", db_res_json);

                        let risk_res_data = RiskEngineResponse {
                            order: Some(current_order),
                            command: "cancel-order".to_string(),
                            fill: None,
                        };

                        let risk_res_json =
                            serde_json::to_string(&risk_res_data).expect("Failed to convert json");

                        risk_engine_pub_client
                            .lpush::<_, _, ()>("matching-to-risk-pub-queue", risk_res_json);
                    }
                    Err(_) => {
                        println!("failed to get order");
                        let res_data = EngineResponse {
                            request_id: request_id.clone(),
                            order_id: order_id.to_string(),
                            filled_quantity: 0,
                            status: NotFound,
                            price: 0,
                            message: "Order not found".to_string(),
                            data: None,
                        };

                        let res_json: String =
                            serde_json::to_string(&res_data).expect("Failed to convert JSON");

                        let res_queue: String = format!("response-queue-{backend_id}-{request_id}");

                        publish_client.lpush::<_, _, ()>(res_queue, res_json);
                    }
                }
            }
        }
        EngineCommand::GetOrder => {
            let backend_id = eng_req.BACKEND_ID.clone();
            let request_id = eng_req.request_id.clone();

            let payload = match eng_req.payload.clone() {
                Payload::Order(order_payload) => Err(Error::msg("not valid payload")),
                Payload::GetOrCancelOrder(order) => Ok(order),
                Payload::GetDepth(_) => Err(Error::msg("not valid payload")),
            };

            if payload.is_ok() {
                let current_pay = payload.unwrap();
                let order_id = current_pay.order_id;
                let symbol = current_pay.symbol;

                let orderbook = orderbooks.get_mut(&symbol).unwrap();

                let order = ORDERBOOK::get_order(&orderbook, order_id.to_string());

                match order {
                    Ok(o) => {
                        let res_data = EngineResponse {
                            request_id: request_id.clone(),
                            order_id: order_id.to_string(),
                            filled_quantity: 0,
                            status: Accepted,
                            price: 0,
                            message: "Order sitting on book".to_string(),
                            data: Some(o),
                        };

                        let res_json: String =
                            serde_json::to_string(&res_data).expect("Failed to convert JSON");

                        let res_queue: String = format!("response-queue-{backend_id}-{request_id}");

                        publish_client.lpush::<_, _, ()>(res_queue, res_json);
                    }
                    Err(_) => {
                        println!("failed to get order");
                        let res_data = EngineResponse {
                            request_id: request_id.clone(),
                            order_id: order_id.to_string(),
                            filled_quantity: 0,
                            status: NotFound,
                            price: 0,
                            message: "Order not found".to_string(),
                            data: None,
                        };

                        let res_json: String =
                            serde_json::to_string(&res_data).expect("Failed to convert JSON");

                        let res_queue: String = format!("response-queue-{backend_id}-{request_id}");

                        publish_client.lpush::<_, _, ()>(res_queue, res_json);
                    }
                }
            }
        }
        EngineCommand::GetDepth => {
            let backend_id = eng_req.BACKEND_ID.clone();
            let request_id = eng_req.request_id.clone();

            let payload = match eng_req.payload.clone() {
                Payload::Order(_) => Err(Error::msg("not valid payload")),
                Payload::GetOrCancelOrder(_) => Err(Error::msg("not valid payload")),
                Payload::GetDepth(txt) => Ok(txt),
            };
            let symbol = &payload.unwrap();

            let orderbook = orderbooks.get_mut(symbol).unwrap();
            let limit = 100;
            let depth = ORDERBOOK::get_depth(&orderbook, limit);

            let res_data = DepthResponse {
                request_id: request_id.clone(),
                symbol: "".to_string(),
                asks: depth.asks,
                bids: depth.bids,
                timestamp: depth.timestamp,
            };
            let res_json: String =
                serde_json::to_string(&res_data).expect("Failed to convert JSON");

            let res_queue: String = format!("response-queue-{backend_id}-{request_id}");

            publish_client.lpush::<_, _, ()>(res_queue, res_json);
        }

        EngineCommand::GetBalance => todo!(),
        EngineCommand::ForcedLiquidate => todo!(),
        EngineCommand::ClosePosition => todo!(),
    }
}

fn process_liquidation(
    eng_req: EngineRequest,
    orderbooks: &mut HashMap<String, ORDERBOOK>,
    publish_client: &mut Connection,
    db_worker_client: &mut Connection,
    risk_engine_pub_client: &mut Connection,
) {
    match EngineCommand::from_str(&eng_req.command).unwrap() {
        CreateOrder => todo!(),
        CancelOrder => todo!(),
        GetOrder => todo!(),
        GetDepth => todo!(),
        GetBalance => todo!(),
        ForcedLiquidate => {
            let backend_id = eng_req.BACKEND_ID.clone();
            let request_id = eng_req.request_id.clone();

            let payload = match eng_req.payload.clone() {
                Payload::Order(order_payload) => Ok(order_payload),
                Payload::GetOrCancelOrder(_) => Err(Error::msg("not valid payload")),
                Payload::GetDepth(_) => Err(Error::msg("not valid payload")),
            };

            if payload.is_ok() {
                let payload = payload.unwrap();

                let open_order = OpenOrder {
                    backend_id: backend_id.clone(),
                    request_id: request_id.clone(),
                    order_id: Uuid::new_v4().to_string(),
                    user_id: payload.user_id.to_string(),
                    order_type: OrderType::from_str(&payload.r#type).unwrap(),
                    side: Side::from_str(&payload.side).unwrap(),
                    symbol: payload.symbol,
                    quantity: payload.quantity.parse().unwrap(),
                    price: payload.price.parse().unwrap(),
                    created_at: Utc::now().timestamp_millis(),
                };
                let mut orderbook = orderbooks.get_mut(&open_order.symbol).unwrap();
                let ord = ORDERBOOK::create_order(&mut orderbook, open_order.clone());

                match ord {
                    Ok(order_detail) => {
                        let payload_res_data = EngineResponse {
                            request_id: request_id.clone(),
                            order_id: open_order.order_id,
                            filled_quantity: order_detail.filled_quantity,
                            status: order_detail.status,
                            price: open_order.price,
                            message: "Order created successfully".to_string(),
                            data: None,
                        };
                        let fill = order_detail.fill;

                        if let Some(current_fill) = fill {
                            let payload_res = DBPayload::Engine(payload_res_data.clone());

                            let db_res_data = DBWorkerResponse {
                                request_id: Some(request_id.clone()),
                                fill: Some(current_fill.clone()),
                                payload: payload_res,
                                request_type: Some(OrderRequestType::Perp),
                                order_id: order_detail.order_id,
                                command: "create-fill".to_string(),
                                order: None,
                            };

                            let db_res_json = serde_json::to_string(&db_res_data)
                                .expect("Failed to convert to json");

                            db_worker_client.lpush::<_, _, ()>("settlement-queue", db_res_json);

                            let risk_res_data = RiskEngineResponse {
                                fill: Some(current_fill),
                                command: "fill-order".to_string(),
                                order: None,
                            };

                            let risk_res_json = serde_json::to_string(&risk_res_data)
                                .expect("Failed to convert json");

                            risk_engine_pub_client
                                .lpush::<_, _, ()>("matching-to-risk-pub-queue", risk_res_json);
                        }

                        let res_json: String = serde_json::to_string(&payload_res_data)
                            .expect("Failed to convert JSON");

                        let res_queue: String = format!("response-queue-{backend_id}-{request_id}");

                        publish_client.lpush::<_, _, ()>(res_queue, res_json);
                    }
                    Err(e) => {
                        let res_data = EngineResponse {
                            request_id: request_id.clone(),
                            order_id: open_order.order_id,
                            filled_quantity: 0,
                            status: Rejected,
                            price: open_order.price,
                            message: e.to_string(),
                            data: None,
                        };

                        let res_json: String =
                            serde_json::to_string(&res_data).expect("Failed to convert JSON");

                        let res_queue: String = format!("response-queue-{backend_id}-{request_id}");

                        publish_client.lpush::<_, _, ()>(res_queue, res_json);
                    }
                }
            }
        }
        EngineCommand::ClosePosition => todo!(),
    }
}

fn process_risk(
    eng_req: EngineRequest,
    orderbooks: &mut HashMap<String, ORDERBOOK>,
    publish_client: &mut Connection,
    db_worker_client: &mut Connection,
    risk_engine_pub_client: &mut Connection,
) {
    match EngineCommand::from_str(&eng_req.command).unwrap() {
        CreateOrder => {
            let backend_id = eng_req.BACKEND_ID.clone();
            let request_id = eng_req.request_id.clone();

            let payload = match eng_req.payload.clone() {
                Payload::Order(order_payload) => Ok(order_payload),
                Payload::GetOrCancelOrder(_) => Err(Error::msg("not valid payload")),
                Payload::GetDepth(_) => Err(Error::msg("not valid payload")),
            };

            if payload.is_ok() {
                let payload = payload.unwrap();

                let open_order = OpenOrder {
                    backend_id: backend_id.clone(),
                    request_id: request_id.clone(),
                    order_id: Uuid::new_v4().to_string(),
                    user_id: payload.user_id.to_string(),
                    order_type: OrderType::from_str(&payload.r#type).unwrap(),
                    side: Side::from_str(&payload.side).unwrap(),
                    symbol: payload.symbol,
                    quantity: payload.quantity.parse().unwrap(),
                    price: payload.price.parse().unwrap(),
                    created_at: Utc::now().timestamp_millis(),
                };
                let mut orderbook = orderbooks.get_mut(&open_order.symbol).unwrap();
                let ord = ORDERBOOK::create_order(&mut orderbook, open_order.clone());

                match ord {
                    Ok(order_detail) => {
                        let payload_res_data = EngineResponse {
                            request_id: request_id.clone(),
                            order_id: open_order.order_id,
                            filled_quantity: order_detail.filled_quantity,
                            status: order_detail.status,
                            price: open_order.price,
                            message: "Order created successfully".to_string(),
                            data: None,
                        };
                        let fill = order_detail.fill;

                        if fill.is_some() {
                            let current_fill = fill.unwrap();
                            let payload_res = DBPayload::Engine(payload_res_data.clone());

                            let db_res_data = DBWorkerResponse {
                                request_id: Some(request_id.clone()),
                                fill: Some(current_fill.clone()),
                                payload: payload_res,
                                request_type: Some(OrderRequestType::Perp),
                                order_id: order_detail.order_id,
                                command: "create-fill".to_string(),
                                order: None,
                            };

                            let db_res_json = serde_json::to_string(&db_res_data)
                                .expect("Failed to convert to json");

                            db_worker_client.lpush::<_, _, ()>("settlement-queue", db_res_json);

                            let risk_res_data = RiskEngineResponse {
                                fill: Some(current_fill),
                                command: "fill-order".to_string(),
                                order: None,
                            };

                            let risk_res_json = serde_json::to_string(&risk_res_data)
                                .expect("Failed to convert json");

                            risk_engine_pub_client
                                .lpush::<_, _, ()>("matching-to-risk-pub-queue", risk_res_json);
                        }

                        let res_json: String = serde_json::to_string(&payload_res_data)
                            .expect("Failed to convert JSON");

                        let res_queue: String = format!("response-queue-{backend_id}-{request_id}");

                        publish_client.lpush::<_, _, ()>(res_queue, res_json);
                    }
                    Err(e) => {
                        let res_data = EngineResponse {
                            request_id: request_id.clone(),
                            order_id: open_order.order_id,
                            filled_quantity: 0,
                            status: Rejected,
                            price: open_order.price,
                            message: e.to_string(),
                            data: None,
                        };

                        let res_json: String =
                            serde_json::to_string(&res_data).expect("Failed to convert JSON");

                        let res_queue: String = format!("response-queue-{backend_id}-{request_id}");

                        publish_client.lpush::<_, _, ()>(res_queue, res_json);
                    }
                }
            }
        }
        EngineCommand::CancelOrder => todo!(),
        EngineCommand::GetOrder => todo!(),
        EngineCommand::GetDepth => todo!(),
        EngineCommand::GetBalance => todo!(),
        EngineCommand::ForcedLiquidate => todo!(),
        EngineCommand::ClosePosition => {
            let backend_id = eng_req.BACKEND_ID.clone();
            let request_id = eng_req.request_id.clone();

            let payload = match eng_req.payload.clone() {
                Payload::Order(order_payload) => Ok(order_payload),
                Payload::GetOrCancelOrder(_) => Err(Error::msg("not valid payload")),
                Payload::GetDepth(_) => Err(Error::msg("not valid payload")),
            };

            if payload.is_ok() {
                let payload = payload.unwrap();

                let open_order = OpenOrder {
                    backend_id: backend_id.clone(),
                    request_id: request_id.clone(),
                    order_id: Uuid::new_v4().to_string(),
                    user_id: payload.user_id.to_string(),
                    order_type: OrderType::from_str(&payload.r#type).unwrap(),
                    side: Side::from_str(&payload.side).unwrap(),
                    symbol: payload.symbol,
                    quantity: payload.quantity.parse().unwrap(),
                    price: payload.price.parse().unwrap(),
                    created_at: Utc::now().timestamp_millis(),
                };
                let mut orderbook = orderbooks.get_mut(&open_order.symbol).unwrap();
                let ord = ORDERBOOK::create_order(&mut orderbook, open_order.clone());

                match ord {
                    Ok(order_detail) => {
                        let payload_res_data = EngineResponse {
                            request_id: request_id.clone(),
                            order_id: open_order.order_id,
                            filled_quantity: order_detail.filled_quantity,
                            status: order_detail.status,
                            price: open_order.price,
                            message: "Order created successfully".to_string(),
                            data: None,
                        };
                        let fill = order_detail.fill;

                        if fill.is_some() {
                            let current_fill = fill.unwrap();
                            let payload_res = DBPayload::Engine(payload_res_data.clone());

                            let db_res_data = DBWorkerResponse {
                                request_id: Some(request_id.clone()),
                                fill: Some(current_fill.clone()),
                                payload: payload_res,
                                request_type: Some(OrderRequestType::Perp),
                                order_id: order_detail.order_id,
                                command: "close-position".to_string(),
                                order: None,
                            };

                            let db_res_json = serde_json::to_string(&db_res_data)
                                .expect("Failed to convert to json");

                            db_worker_client.lpush::<_, _, ()>("settlement-queue", db_res_json);

                            let risk_res_data = RiskEngineResponse {
                                fill: Some(current_fill),
                                command: "fill-order".to_string(),
                                order: None,
                            };

                            let risk_res_json = serde_json::to_string(&risk_res_data)
                                .expect("Failed to convert json");

                            risk_engine_pub_client
                                .lpush::<_, _, ()>("matching-to-risk-pub-queue", risk_res_json);
                        }

                        let res_json: String = serde_json::to_string(&payload_res_data)
                            .expect("Failed to convert JSON");

                        let res_queue: String = format!("response-queue-{backend_id}-{request_id}");

                        publish_client.lpush::<_, _, ()>(res_queue, res_json);
                    }
                    Err(e) => {
                        let res_data = EngineResponse {
                            request_id: request_id.clone(),
                            order_id: open_order.order_id,
                            filled_quantity: 0,
                            status: Rejected,
                            price: open_order.price,
                            message: e.to_string(),
                            data: None,
                        };

                        let res_json: String =
                            serde_json::to_string(&res_data).expect("Failed to convert JSON");

                        let res_queue: String = format!("response-queue-{backend_id}-{request_id}");

                        publish_client.lpush::<_, _, ()>(res_queue, res_json);
                    }
                }
            }
        }
    }
}
