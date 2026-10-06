use crate::orderbook::*;
use std::collections::{BTreeMap, HashMap, VecDeque};

mod order;
mod orderbook;

fn main() {
    println!("Hello, world!");

    //define create order, cancel order, get order, get depth
    //define request, command, response, engine status, engine response,

    //orderbook_mock();

    let mut orderbook = Orderbook {
        asks: BTreeMap::new(),
        bids: BTreeMap::new(),
    };

    let mut balances: HashMap<String, Balance> = HashMap::new();
    // let mut orders: HashMap<String, OpenOrder> = HashMap::new();

    let mut fills: HashMap<String, Fill> = HashMap::new();
}
