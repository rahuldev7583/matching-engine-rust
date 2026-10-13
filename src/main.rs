use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    env,
};

use anyhow::{Error, Result};
use chrono::Utc;
use dotenvy::dotenv;
use redis::{Connection, RedisError};
use uuid::Uuid;

use crate::{
    EngineStatus::{Down, Pending, Ready},
    order::Orderbook,
    orderbook::{
        ORDERBOOK, OpenOrder, OrderStatus,
        OrderType::Limit,
        Side::{Buy, Sell},
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

pub enum EngineCommand {
    CreateOrder,
    CancelOrder,
    GetOrder,
    GetDepth,
    GetBalance,
}

pub struct EngineRequest {
    pub request_id: String,
    pub payload: String,
    pub backend_id: String,
    pub command: EngineCommand,
}

pub struct EngineResponse {
    pub request_id: String,
    pub backend_id: String,
    pub order_id: String,
    pub filled_quantity: u64,
    pub status: OrderStatus,
    pub price: u64,
    pub message: String,
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

fn main() {
    println!("Hello, world!");
    let mut engine_status = EngineStatus::new();

    dotenv().ok();

    let REDIS_URL = env::var("REDIS_URL").expect("REDIS_URL is not defined");

    println!("REDIS_URL:{}", &REDIS_URL);

    if engine_status == Down || engine_status == Pending {
        let mut client = new_redis_client(REDIS_URL.clone()).unwrap();

        println!("client connected sucessfully");

        let mut publishClient = new_redis_client(REDIS_URL.clone()).unwrap();

        println!("publishClient connected sucessfully");

        let mut dbWorkerClient = new_redis_client(REDIS_URL.clone()).unwrap();

        println!("dbWorkerClient connected sucessfully");

        let mut riskEngineClient = new_redis_client(REDIS_URL.clone()).unwrap();

        println!("riskEngineClient connected sucessfully");

        let mut riskEnginePubClient = new_redis_client(REDIS_URL.clone()).unwrap();

        println!("riskEnginePubClient connected sucessfully");

        let mut liquidationClient = new_redis_client(REDIS_URL.clone()).unwrap();

        println!("liquidationClient connected sucessfully");

        engine_status = Ready;
    } else {
        println!("engine is Ready");
    }

    //define create order, cancel order, get order, get depth
    //define request, command, response, engine status, engine response,

    //orderbook_mock();

    let mut market_books: HashMap<String, ORDERBOOK> = HashMap::new();

    market_books.insert("BTC_USD".to_string(), ORDERBOOK::new());

    println!("market_books: {:?}", market_books);

    let btcBOOK = market_books.get("BTC_USD").unwrap();

    // let mut balances: HashMap<String, Balance> = HashMap::new();
    // let mut orders: HashMap<String, OpenOrder> = HashMap::new();

    println!("hi");

    println!("rust matching engine");

    // let mut fills: HashMap<String, Fill> = HashMap::new();
}
