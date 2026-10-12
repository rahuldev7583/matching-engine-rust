//Order , OpenOrder, Fill, Market, Side, order_type, orderbook

use std::collections::{BTreeMap, HashMap, VecDeque};

use strum::{Display, EnumString};

use crate::orderbook::{OrderType::Limit, Side::Buy};

#[derive(Debug, Display, EnumString, PartialEq, Eq, Clone)]
pub enum Side {
    #[strum(serialize = "buy")]
    Buy,
    #[strum(serialize = "sell")]
    Sell,
}

#[derive(Debug, Display, EnumString, PartialEq, Eq, Clone)]
pub enum OrderType {
    #[strum(serialize = "limit")]
    Limit,
    #[strum(serialize = "Market")]
    Market,
}

#[derive(Debug, EnumString, PartialEq)]
pub enum OrderStatus {
    #[strum(serialize = "accepted")]
    Accepted,
    #[strum(serialize = "rejected")]
    Rejected,
    #[strum(serialize = "fully filled")]
    FullyFilled,
    #[strum(serialize = "partially filled")]
    PartiallyFilled,
    #[strum(serialize = "cancelled")]
    Cancelled,
    #[strum(serialize = "self-trade-prevented")]
    SelfTradePrevented,
}

pub struct Balance {
    pub locked: u64,
    pub available: u64,
}
#[derive(Debug, Clone)]
pub struct Order {
    pub request_id: String,
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

impl Order {
    pub fn new(
        request_id: String,
        user_id: String,
        order_id: String,
        order_type: OrderType,
        side: Side,
        symbol: String,
        quantity: u64,
        price: u64,
        created_at: u32,
        fills: Vec<Fill>,
    ) -> Self {
        Self {
            request_id,
            order_id,
            user_id,
            order_type,
            side,
            symbol,
            quantity,
            price,
            fills,
            created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenOrder {
    pub request_id: String,
    pub user_id: String,
    pub order_id: String,
    pub order_type: OrderType,
    pub side: Side,
    pub symbol: String,
    pub quantity: u64,
    pub price: u64,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct Fill {
    pub fill_id: String,
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

impl Fill {
    pub fn new(
        fill_id: String,
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
    ) -> Self {
        Self {
            fill_id,
            buy_user_id,
            buy_order_id,
            sell_user_id,
            sell_order_id,
            filled_quantity,
            order_type,
            side,
            symbol,
            quantity,
            price,
            created_at,
        }
    }
}

#[derive(Debug)]
pub struct ORDERBOOK {
    pub asks: BTreeMap<u64, VecDeque<OpenOrder>>,
    pub bids: BTreeMap<u64, VecDeque<OpenOrder>>,
    pub orders: HashMap<String, OpenOrder>,
}

impl ORDERBOOK {
    pub fn new() -> Self {
        Self {
            asks: BTreeMap::new(),
            bids: BTreeMap::new(),
            orders: HashMap::new(),
        }
    }
}

pub fn orderbook_mock() {
    let mut orderbook = ORDERBOOK {
        asks: BTreeMap::new(),
        bids: BTreeMap::new(),

        orders: HashMap::new(),
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
