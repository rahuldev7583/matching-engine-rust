//Order , OpenOrder, Fill, Market, Side, order_type, orderbook

use std::collections::{BTreeMap, VecDeque};

use strum::{Display, EnumString};

use crate::orderbook::{OrderType::Limit, Side::Buy};

#[derive(Debug, Display, EnumString)]
pub enum Side {
    #[strum(serialize = "buy")]
    Buy,
    #[strum(serialize = "sell")]
    Sell,
}

#[derive(Debug, Display, EnumString)]
pub enum OrderType {
    #[strum(serialize = "limit")]
    Limit,
    #[strum(serialize = "Market")]
    Market,
}

#[derive(Debug, Display, EnumString)]
pub enum OrderStatus {
    #[strum(serialize = "filled")]
    Filled,
    #[strum(serialize = "partially-filled")]
    PartiallyFilled,
    #[strum(serialize = "cancelled")]
    Cancelled,
}

pub struct Balance {
    locked: u64,
    available: u64,
}

pub struct Order {
    request_id: u32,
    order_id: String,
    user_id: String,
    order_type: OrderType,
    side: Side,
    symbol: String,
    quantity: u64,
    price: u64,

    fills: Vec<Fill>,
    created_at: u32,
}

#[derive(Debug)]
pub struct OpenOrder {
    request_id: String,
    user_id: String,
    order_id: String,
    order_type: OrderType,
    side: Side,
    symbol: String,
    quantity: u64,
    price: u64,
    created_at: u32,
}

pub struct Fill {
    buy_user_id: String,
    buy_order_id: String,
    sell_user_id: String,
    sell_order_id: String,

    filled_quantity: u64,

    order_type: OrderType,
    side: Side,
    symbol: String,
    quantity: u64,
    price: u64,
    created_at: u32,
}

#[derive(Debug)]
pub struct Orderbook {
    asks: BTreeMap<u64, VecDeque<OpenOrder>>,
    bids: BTreeMap<u64, VecDeque<OpenOrder>>,
}

pub fn orderbook_mock() {
    let mut orderbook = Orderbook {
        asks: BTreeMap::new(),
        bids: BTreeMap::new(),
    };

    println!("orderbook: {:?}", orderbook);
    let price: u64 = 10;

    let orders = orderbook.asks.get(&10);

    match orders {
        Some(o) => {
            println!("orderbook does have orders at this price, do the matching logic")
        }
        None => {
            let order = OpenOrder {
                request_id: "123".to_string(),
                user_id: "345".to_string(),
                order_id: "order_12".to_string(),
                order_type: Limit,
                side: Buy,
                symbol: "BTC".to_string(),
                quantity: 10,
                price,
                created_at: 173333,
            };
            let mut order_arr: VecDeque<OpenOrder> = VecDeque::new();

            order_arr.push_front(order);

            orderbook.asks.insert(price, order_arr);
        }
    }

    println!("orderbook: {:?}", orderbook);
}
