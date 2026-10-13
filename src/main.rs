use std::collections::{BTreeMap, HashMap, VecDeque};

use chrono::Utc;
use uuid::Uuid;

use crate::{
    order::Orderbook,
    orderbook::{
        ORDERBOOK, OpenOrder,
        OrderType::Limit,
        Side::{Buy, Sell},
    },
};

mod order;
mod orderbook;
#[cfg(test)]
mod test;

fn main() {
    println!("Hello, world!");

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
