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
    pub locked: u64,
    pub available: u64,
}

pub struct Order {
    pub request_id: u32,
    pub order_id: String,
    pub user_id: String,
    pub order_type: OrderType,
    pub side: Side,
    pub symbol: String,
    pub quantity: u64,
    pub price: u64,

    pub fills: Vec<Fill>,
    pub created_at: u32,
}

#[derive(Debug)]
pub struct OpenOrder {
    pub request_id: String,
    pub user_id: String,
    pub order_id: String,
    pub order_type: OrderType,
    pub side: Side,
    pub symbol: String,
    pub quantity: u64,
    pub price: u64,
    pub created_at: u32,
}

pub struct Fill {
    pub buy_user_id: String,
    pub buy_order_id: String,
    pub sell_user_id: String,
    pub sell_order_id: String,

    pub filled_quantity: u64,

    pub order_type: OrderType,
    pub side: Side,
    pub symbol: String,
    pub quantity: u64,
    pub price: u64,
    pub created_at: u32,
}

#[derive(Debug)]
pub struct Orderbook {
    pub asks: BTreeMap<u64, VecDeque<OpenOrder>>,
    pub bids: BTreeMap<u64, VecDeque<OpenOrder>>,
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
