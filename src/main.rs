use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    env,
};

use anyhow::{Error, Result};
use chrono::Utc;
use dotenvy::dotenv;
use redis::{Commands, Connection, JsonCommands, RedisError};
use serde::{Deserialize, Serialize, de::value::Error as SerdeError};
use serde_json::Error as SerdeJsonError;
use serde_json::from_str;
use strum::EnumString;
use uuid::Uuid;

use crate::{
    EngineCommand::CreateOrder,
    EngineStatus::{Down, Pending, Ready},
    order::{Depth, Orderbook},
    orderbook::{
        Fill, ORDERBOOK, OpenOrder,
        OrderStatus::{self, Accepted, Cancelled, Rejected},
        OrderType::{self, Limit},
        Side::{self, Buy, Sell},
    },
};

mod order;
mod orderbook;
#[cfg(test)]
mod test;

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
    pub risk_engine_client: Connection,
    pub risk_engine_pub_client: Connection,
    pub liquidation_client: Connection,
}

impl RedisServics {
    pub fn new(url: String) -> Self {
        Self {
            client: new_redis_client(url.clone()).unwrap(),
            publish_client: new_redis_client(url.clone()).unwrap(),
            db_worker_client: new_redis_client(url.clone()).unwrap(),
            risk_engine_client: new_redis_client(url.clone()).unwrap(),
            risk_engine_pub_client: new_redis_client(url.clone()).unwrap(),
            liquidation_client: new_redis_client(url.clone()).unwrap(),
        }
    }
}

#[derive(Deserialize, Debug, PartialEq, Serialize, EnumString)]
pub enum EngineCommand {
    CreateOrder,
    CancelOrder,
    GetOrder,
    GetDepth,
    GetBalance,

    ForcedLiquidation,
    ClosedPosition,
}

#[derive(Deserialize, Debug, PartialEq, Serialize, Clone)]
pub struct OrderPayload {
    backend_id: String,
    request_id: String,
    user_id: String,
    order_type: OrderType,
    side: Side,
    quantity: u64,
    price: u64,
    symbol: String,
}

#[derive(Deserialize, Debug, PartialEq, Serialize, Clone)]
#[serde(untagged)]
pub enum Payload {
    Object(OrderPayload),
    Text(String),
}

//TODO: engine request payload
#[derive(Deserialize, Debug, Serialize)]
pub struct EngineRequest {
    pub request_id: String,
    pub payload: Payload,
    pub backend_id: String,
    pub command: EngineCommand,
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
    pub backend_id: String,
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
    CancleOrder(DBPayloadRes),
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

fn new_redis_client(url: String) -> Result<Connection, Error> {
    match redis::Client::open(url) {
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

//TODO:
//CREATE ORDER =>

//      ii. if full filled => db_worker_client with =>{ fill, payload, request_type: request_type, order_id, command: command || 'create-fill' }
//                          => riskEngPubclient with => {fill, command: "create-fill"}
//      iii. After looping all orders =>  publishclient with => EngineResponse

//CANCEL ORDER =>
//      i. publishclient with => EngineResponse
//      ii. riskEngPubclient => {order, command: "cancel-order"}
//      iii. dbWorkerclient => { request_id, filled_quantity: 0, message: '', ...order, fill: {}, payload: { symbol: order.symbol }, command: "cancel-order" }

//GET ORDER =>
//         publishclient =>  request_id,         data: order,         status: 'order pending',         message: 'Order is Sitting on orderbook',         order_id: '',         price: 0,         filled_quantity: 0

//GET DEPTH =>
//          publishclient => request_id,         symbol: payload,         bids: bids,         asks: asks,         timestamp: Date.now()

fn main() {
    println!("Hello, world!");
    let mut engine_status = EngineStatus::new();

    dotenv().ok();

    let REDIS_URL = env::var("REDIS_URL").expect("REDIS_URL is not defined");

    println!("REDIS_URL:{}", &REDIS_URL);
    let redis_clients = RedisServics::new(REDIS_URL);

    let RedisServics {
        mut client,
        mut publish_client,
        mut db_worker_client,
        mut risk_engine_client,
        mut risk_engine_pub_client,
        mut liquidation_client,
    } = redis_clients;

    let mut orderbook = ORDERBOOK::new();

    let incoming_request: Result<String, RedisError> = client.brpop("incoming_request", 0.1);

    match incoming_request {
        Ok(req) => {
            let parsed_req: Result<EngineRequest, SerdeJsonError> = serde_json::from_str(&req);

            println!("parsed_req: {:?}", parsed_req);

            match parsed_req {
                Ok(eng_req) => match eng_req.command {
                    CreateOrder => {
                        //TODO:

                        let backend_id = eng_req.backend_id.clone();
                        let request_id = eng_req.request_id.clone();

                        let payload = match eng_req.payload.clone() {
                            Payload::Object(order_payload) => Ok(order_payload),
                            Payload::Text(_) => Err(Error::msg("not valid payload")),
                        };

                        if payload.is_ok() {
                            let payload = payload.unwrap();

                            let open_order = OpenOrder {
                                backend_id: backend_id.clone(),
                                request_id: request_id.clone(),
                                order_id: Uuid::new_v4().to_string(),
                                user_id: payload.user_id,
                                order_type: payload.order_type,
                                side: payload.side,
                                symbol: payload.symbol,
                                quantity: payload.quantity,
                                price: payload.price,
                                created_at: Utc::now().timestamp_millis(),
                            };
                            let ord = ORDERBOOK::create_order(&mut orderbook, open_order.clone());

                            match ord {
                                Ok(order_detail) => {
                                    let payload_res_data = EngineResponse {
                                        request_id: request_id.clone(),
                                        backend_id: backend_id.clone(),
                                        order_id: open_order.order_id,
                                        filled_quantity: order_detail.filled_quantity,
                                        status: order_detail.status,
                                        price: open_order.price,
                                        message: "Order created successfully".to_string(),

                                        data: None,
                                    };
                                    let fill = order_detail.fill;

                                    //TODO: i want paylod to be as EngineResponse

                                    if fill.is_some() {
                                        let payload_res = DBPayload::Engine(payload_res_data);

                                        let db_res_data = DBWorkerResponse {
                                            request_id: Some(request_id),
                                            fill: Some(fill.unwrap()),
                                            payload: payload_res,
                                            request_type: todo!(),
                                            order_id: order_detail.order_id,
                                            command: "create-fill".to_string(),
                                            order: None,
                                        };

                                        let db_res_json = serde_json::to_string(&db_res_data)
                                            .expect("Failed to convet to json");

                                        db_worker_client
                                            .lpush::<_, _, ()>("settlement-queue", db_res_json);

                                        let risk_res_data = RiskEngineResponse {
                                            fill: Some(fill.unwrap()),
                                            command: "fill-order".to_string(),
                                            order: None,
                                        };

                                        let risk_res_json = serde_json::to_string(&risk_res_data)
                                            .expect("Failed to convert json");

                                        risk_engine_pub_client.lpush::<_, _, ()>(
                                            "matching-to-risk-pub-queue",
                                            risk_res_json,
                                        );
                                    }

                                    let res_json: String = serde_json::to_string(&payload_res_data)
                                        .expect("Failed to convert JSON");

                                    let res_queue: String = format!("response-queue-{backend_id}");

                                    publish_client.lpush::<_, _, ()>(res_queue, res_json);
                                }
                                Err(e) => {
                                    let res_data = EngineResponse {
                                        request_id: request_id.clone(),
                                        backend_id: backend_id.clone(),
                                        order_id: open_order.order_id,
                                        filled_quantity: 0,
                                        status: Rejected,
                                        price: open_order.price,
                                        message: e.to_string(),

                                        data: None,
                                    };

                                    let res_json: String = serde_json::to_string(&res_data)
                                        .expect("Failed to convert JSON");

                                    let res_queue: String = format!("response-queue-{backend_id}");

                                    publish_client.lpush::<_, _, ()>(res_queue, res_json);
                                }
                            }
                        }
                    }
                    EngineCommand::CancelOrder => {
                        let backend_id = eng_req.backend_id.clone();
                        let request_id = eng_req.request_id.clone();

                        let payload = match eng_req.payload.clone() {
                            Payload::Object(order_payload) => Err(Error::msg("not valid payload")),
                            Payload::Text(order_id) => Ok(order_id),
                        };

                        if let Ok(OrderPayload) = &payload {
                            let order_id = payload.unwrap();
                            let ord = ORDERBOOK::cancel_order(&mut orderbook, order_id.clone());

                            //TODO: here i need, EngineResponse, order, request_id, symbol

                            let res_data = EngineResponse {
                                request_id: request_id.clone(),
                                backend_id: backend_id.clone(),
                                order_id: order_id,
                                filled_quantity: 0,
                                status: Cancelled,
                                price: 0,
                                message: "Order cancelled".to_string(),

                                data: None,
                            };

                            let res_json: String =
                                serde_json::to_string(&res_data).expect("Failed to convert JSON");

                            let res_queue: String = format!("response-queue-{backend_id}");

                            publish_client.lpush::<_, _, ()>(res_queue, res_json);

                            if ord.is_ok() {
                                //TODO: i want payload to be order symbol string

                                let payload_res = DBPayload::CancleOrder(DBPayloadRes {
                                    symbol: ord.unwrap().symbol,
                                });

                                let db_res_data = DBWorkerResponse {
                                    request_id: Some(request_id.clone()),

                                    fill: None,
                                    payload: payload_res,
                                    request_type: todo!(),
                                    order: Some(ord.unwrap()),
                                    order_id: ord.unwrap().order_id,
                                    command: "cancel-order".to_string(),
                                };

                                let db_res_json = serde_json::to_string(&db_res_data)
                                    .expect("Failed to convet to json");

                                db_worker_client.lpush::<_, _, ()>("settlement-queue", db_res_json);

                                let risk_res_data = RiskEngineResponse {
                                    order: Some(ord.unwrap()),
                                    command: "cancel-order".to_string(),
                                    fill: None,
                                };

                                let risk_res_json = serde_json::to_string(&risk_res_data)
                                    .expect("Failed to convert json");

                                risk_engine_pub_client
                                    .lpush::<_, _, ()>("matching-to-risk-pub-queue", risk_res_json);
                            }
                        }
                    }
                    EngineCommand::GetOrder => {
                        let backend_id = eng_req.backend_id.clone();
                        let request_id = eng_req.request_id.clone();

                        let payload = match eng_req.payload.clone() {
                            Payload::Object(order_payload) => Err(Error::msg("not valid payload")),
                            Payload::Text(order_id) => Ok(order_id),
                        };

                        if payload.is_ok() {
                            let order_id = &payload.unwrap();

                            let order = ORDERBOOK::get_order(&orderbook, order_id.to_string());

                            match order {
                                Ok(o) => {
                                    let res_data = EngineResponse {
                                        request_id: request_id.clone(),
                                        backend_id: backend_id.clone(),
                                        order_id: order_id.to_string(),
                                        filled_quantity: 0,
                                        status: Accepted,
                                        price: 0,
                                        message: "Order sitting on book".to_string(),

                                        data: Some(o),
                                    };

                                    let res_json: String = serde_json::to_string(&res_data)
                                        .expect("Failed to convert JSON");

                                    let res_queue: String = format!("response-queue-{backend_id}");

                                    publish_client.lpush::<_, _, ()>(res_queue, res_json);
                                }
                                Err(e) => {}
                            }
                        }
                    }
                    EngineCommand::GetDepth => {
                        let backend_id = eng_req.backend_id.clone();
                        let request_id = eng_req.request_id.clone();

                        let limit = 100;
                        let depth = ORDERBOOK::get_depth(&orderbook, limit);

                        let res_data = DepthResponse {
                            request_id,
                            symbol: "".to_string(),
                            asks: depth.asks,
                            bids: depth.bids,
                            timestamp: depth.timestamp,
                        };
                        let res_json: String =
                            serde_json::to_string(&res_data).expect("Failed to convert JSON");

                        let res_queue: String = format!("response-queue-{backend_id}");

                        publish_client.lpush::<_, _, ()>(res_queue, res_json);
                    }
                    EngineCommand::GetBalance => todo!(),
                    EngineCommand::ForcedLiquidation => todo!(),
                    EngineCommand::ClosedPosition => todo!(),
                },
                Err(e) => println!("failed to parse request"),
            }
        }
        Err(e) => println!("failed to get request: {}", e),
    }

    let liquidation_request: Result<String, RedisError> =
        liquidation_client.brpop("liquidation-to-matching-eng", 0.1);

    match liquidation_request {
        Ok(req) => {
            let parsed_liq_req: Result<EngineRequest, SerdeJsonError> = serde_json::from_str(&req);

            println!("parsed_liq_req: {:?}", parsed_liq_req);

            match parsed_liq_req {
                Ok(liq_req) => match liq_req.command {
                    CreateOrder => todo!(),
                    EngineCommand::CancelOrder => todo!(),
                    EngineCommand::GetOrder => todo!(),
                    EngineCommand::GetDepth => todo!(),
                    EngineCommand::GetBalance => todo!(),
                    EngineCommand::ForcedLiquidation => {
                        let backend_id = liq_req.backend_id.clone();
                        let request_id = liq_req.request_id.clone();

                        let payload = match liq_req.payload.clone() {
                            Payload::Object(order_payload) => Ok(order_payload),
                            Payload::Text(_) => Err(Error::msg("not valid payload")),
                        };

                        if payload.is_ok() {
                            let payload = payload.unwrap();

                            let open_order = OpenOrder {
                                backend_id: backend_id.clone(),
                                request_id: request_id.clone(),
                                order_id: Uuid::new_v4().to_string(),
                                user_id: payload.user_id,
                                order_type: payload.order_type,
                                side: payload.side,
                                symbol: payload.symbol,
                                quantity: payload.quantity,
                                price: payload.price,
                                created_at: Utc::now().timestamp_millis(),
                            };
                            let ord = ORDERBOOK::create_order(&mut orderbook, open_order.clone());

                            match ord {
                                Ok(order_detail) => {
                                    let payload_res_data = EngineResponse {
                                        request_id: request_id.clone(),
                                        backend_id: backend_id.clone(),
                                        order_id: open_order.order_id,
                                        filled_quantity: order_detail.filled_quantity,
                                        status: order_detail.status,
                                        price: open_order.price,
                                        message: "Order created successfully".to_string(),

                                        data: None,
                                    };
                                    let fill = order_detail.fill;

                                    //TODO: i want paylod to be as EngineResponse

                                    if fill.is_some() {
                                        let payload_res = DBPayload::Engine(payload_res_data);

                                        let db_res_data = DBWorkerResponse {
                                            request_id: Some(request_id),
                                            fill: Some(fill.unwrap()),
                                            payload: payload_res,
                                            request_type: todo!(),
                                            order_id: order_detail.order_id,
                                            command: "create-fill".to_string(),
                                            order: None,
                                        };

                                        let db_res_json = serde_json::to_string(&db_res_data)
                                            .expect("Failed to convet to json");

                                        db_worker_client
                                            .lpush::<_, _, ()>("settlement-queue", db_res_json);

                                        let risk_res_data = RiskEngineResponse {
                                            fill: Some(fill.unwrap()),
                                            command: "fill-order".to_string(),
                                            order: None,
                                        };

                                        let risk_res_json = serde_json::to_string(&risk_res_data)
                                            .expect("Failed to convert json");

                                        risk_engine_pub_client.lpush::<_, _, ()>(
                                            "matching-to-risk-pub-queue",
                                            risk_res_json,
                                        );
                                    }

                                    let res_json: String = serde_json::to_string(&payload_res_data)
                                        .expect("Failed to convert JSON");

                                    let res_queue: String = format!("response-queue-{backend_id}");

                                    publish_client.lpush::<_, _, ()>(res_queue, res_json);
                                }
                                Err(e) => {
                                    let res_data = EngineResponse {
                                        request_id: request_id.clone(),
                                        backend_id: backend_id.clone(),
                                        order_id: open_order.order_id,
                                        filled_quantity: 0,
                                        status: Rejected,
                                        price: open_order.price,
                                        message: e.to_string(),

                                        data: None,
                                    };

                                    let res_json: String = serde_json::to_string(&res_data)
                                        .expect("Failed to convert JSON");

                                    let res_queue: String = format!("response-queue-{backend_id}");

                                    publish_client.lpush::<_, _, ()>(res_queue, res_json);
                                }
                            }
                        }
                    }
                    EngineCommand::ClosedPosition => todo!(),
                },
                Err(e) => {
                    println!("error occured")
                }
            }
        }
        Err(e) => println!("failed to get request"),
    }

    let incoming_risk_request: Result<String, RedisError> =
        risk_engine_client.brpop("risk-to-matching-eng", 0.1);

    match incoming_risk_request {
        Ok(req) => {
            let parsed_risk_req: Result<EngineRequest, SerdeJsonError> = serde_json::from_str(&req);

            println!("parsed_risk_req: {:?}", parsed_risk_req);

            match parsed_risk_req {
                Ok(risk_req) => match risk_req.command {
                    CreateOrder => {
                        let backend_id = risk_req.backend_id.clone();
                        let request_id = risk_req.request_id.clone();

                        let payload = match risk_req.payload.clone() {
                            Payload::Object(order_payload) => Ok(order_payload),
                            Payload::Text(_) => Err(Error::msg("not valid payload")),
                        };

                        if payload.is_ok() {
                            let payload = payload.unwrap();

                            let open_order = OpenOrder {
                                backend_id: backend_id.clone(),
                                request_id: request_id.clone(),
                                order_id: Uuid::new_v4().to_string(),
                                user_id: payload.user_id,
                                order_type: payload.order_type,
                                side: payload.side,
                                symbol: payload.symbol,
                                quantity: payload.quantity,
                                price: payload.price,
                                created_at: Utc::now().timestamp_millis(),
                            };
                            let ord = ORDERBOOK::create_order(&mut orderbook, open_order.clone());

                            match ord {
                                Ok(order_detail) => {
                                    let payload_res_data = EngineResponse {
                                        request_id: request_id.clone(),
                                        backend_id: backend_id.clone(),
                                        order_id: open_order.order_id,
                                        filled_quantity: order_detail.filled_quantity,
                                        status: order_detail.status,
                                        price: open_order.price,
                                        message: "Order created successfully".to_string(),

                                        data: None,
                                    };
                                    let fill = order_detail.fill;

                                    //TODO: i want paylod to be as EngineResponse

                                    if fill.is_some() {
                                        let payload_res = DBPayload::Engine(payload_res_data);

                                        let db_res_data = DBWorkerResponse {
                                            request_id: Some(request_id),
                                            fill: Some(fill.unwrap()),
                                            payload: payload_res,
                                            request_type: todo!(),
                                            order_id: order_detail.order_id,
                                            command: "create-fill".to_string(),
                                            order: None,
                                        };

                                        let db_res_json = serde_json::to_string(&db_res_data)
                                            .expect("Failed to convet to json");

                                        db_worker_client
                                            .lpush::<_, _, ()>("settlement-queue", db_res_json);

                                        let risk_res_data = RiskEngineResponse {
                                            fill: Some(fill.unwrap()),
                                            command: "fill-order".to_string(),
                                            order: None,
                                        };

                                        let risk_res_json = serde_json::to_string(&risk_res_data)
                                            .expect("Failed to convert json");

                                        risk_engine_pub_client.lpush::<_, _, ()>(
                                            "matching-to-risk-pub-queue",
                                            risk_res_json,
                                        );
                                    }

                                    let res_json: String = serde_json::to_string(&payload_res_data)
                                        .expect("Failed to convert JSON");

                                    let res_queue: String = format!("response-queue-{backend_id}");

                                    publish_client.lpush::<_, _, ()>(res_queue, res_json);
                                }
                                Err(e) => {
                                    let res_data = EngineResponse {
                                        request_id: request_id.clone(),
                                        backend_id: backend_id.clone(),
                                        order_id: open_order.order_id,
                                        filled_quantity: 0,
                                        status: Rejected,
                                        price: open_order.price,
                                        message: e.to_string(),

                                        data: None,
                                    };

                                    let res_json: String = serde_json::to_string(&res_data)
                                        .expect("Failed to convert JSON");

                                    let res_queue: String = format!("response-queue-{backend_id}");

                                    publish_client.lpush::<_, _, ()>(res_queue, res_json);
                                }
                            }
                        }
                    }
                    EngineCommand::CancelOrder => todo!(),
                    EngineCommand::GetOrder => todo!(),
                    EngineCommand::GetDepth => todo!(),
                    EngineCommand::GetBalance => todo!(),
                    EngineCommand::ForcedLiquidation => todo!(),
                    EngineCommand::ClosedPosition => {}
                },
                Err(e) => {
                    println!("error occured")
                }
            }
        }
        Err(e) => println!("failed to get request: {}", e),
    }
}
