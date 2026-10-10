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
mod tests;

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
    //

    println!("hi");

    println!("rust matching engine");

    // let mut fills: HashMap<String, Fill> = HashMap::new();
}

#[test]
fn orderbook_create() {
    let mut market_books: HashMap<String, ORDERBOOK> = HashMap::new();

    market_books.insert("BTC_USD".to_string(), ORDERBOOK::new());

    println!("market_books: {:?}", market_books);

    let mut btcBOOK = market_books.get_mut("BTC_USD").unwrap();
    let buy_order = OpenOrder {
        request_id: "req-123".to_string(),
        user_id: "user-123".to_string(),
        order_id: Uuid::new_v4().to_string(),
        order_type: Limit,
        side: Buy,
        symbol: "BTC_USD".to_string(),
        quantity: 10,
        price: 20,
        created_at: Utc::now().timestamp_millis(),
    };

    ORDERBOOK::create_order(&mut btcBOOK, buy_order.clone());

    let get_buy_ord = ORDERBOOK::get_order(&btcBOOK, buy_order.order_id);

    println!("get_buy_ord: {:?}", get_buy_ord);

    let sell_order = OpenOrder {
        request_id: "req-123".to_string(),
        user_id: "user-123".to_string(),
        order_id: Uuid::new_v4().to_string(),
        order_type: Limit,
        side: Sell,
        symbol: "BTC_USD".to_string(),
        quantity: 10,
        price: 30,
        created_at: Utc::now().timestamp_millis(),
    };

    let sell_ord_status = ORDERBOOK::create_order(&mut btcBOOK, sell_order.clone());

    println!("sell_ord_status: {:?}", sell_ord_status);

    let get_sell_ord = ORDERBOOK::get_order(&btcBOOK, "123".to_string());

    println!("get_sell_ord: {:?}", get_sell_ord);

    println!("btcBOOK: {:?}", btcBOOK);

    let cancel_ord = ORDERBOOK::cancel_order(&mut btcBOOK, sell_order.order_id);
    println!("btcBOOK AFTER cancel: {:?}", btcBOOK);
    let depth = ORDERBOOK::get_depth(&btcBOOK, 10);

    println!("depth: {:?}", depth);
    println!("market_books: {:?}", market_books);
    let rst = 1;
    assert!(1 == rst)
}
