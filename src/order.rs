#![allow(unused)]
//TODO: create order, cancel order, get order, get depth

//TODO: create order
//handle cases:
//  order type => market
//      side buy =>
//                  i. get the lowest price available in the market
//                  ii. ideally order will be fully filed, loop over match order, do the fill logic, if
//                  still unfilled move to next lowest price, until either no order left for
//                  this market or order get fully filled
//
//      side sell =>
//                do the opposite

//  order type => limit
//      side buy =>
//                  i. get the lowest price available in the market
//                  ii. if asked price is equal to or less than lowest price then proceed else order
//                  didn't match, push to orderbook
//                  iii. else order got match, order will be PartiallyFilled or fully filled
//                  iv. loop over match orders until order got filled
//                      =>
//                          I. check if order will get filled by this order only or not, if yes then
//                          do the fill
//                          II. if not, first filled current one, increase fill quantity, remove
//                          current order from book, look for next lowest order, do the same, until
//                          either order got filled or the lowest order price now is more than the
//                          asked order price
//                          NOTE: At one price there are many orders so first loop over until order
//                          got filled, then if not then look for next lowest order
//      side sell =>
//              do the opposite

use std::{
    collections::{BTreeMap, VecDeque},
    iter,
};

use chrono::Utc;

use crate::orderbook::{
    Fill, ORDERBOOK, OpenOrder, Order, OrderType,
    Side::{self, Buy},
};

pub trait Orderbook {
    fn new() -> Self;
    fn create_order(&mut self, order: OpenOrder);
    fn cancel_order(&self, order_id: String);
    fn get_order(&self, order_id: String);
    fn get_depth(&self);
    fn limit_order_matching(&mut self, order: OpenOrder);
}

impl Orderbook for ORDERBOOK {
    fn new() -> Self {
        Self {
            asks: BTreeMap::new(),
            bids: BTreeMap::new(),
        }
    }
    fn create_order(&mut self, order: OpenOrder) {
        println!("create_order");
        println!("order: {:?}", order);

        if order.order_type == OrderType::Market {
            println!("market order");

            if order.side == Side::Buy {
                //TODO: if asks in orderbook is empty throw error

                println!("buy order");
            } else {
                //TODO: if bids in orderbook is empty throw error
                println!("sell order");
            }
        } else {
            println!("limit order");

            if order.side == Side::Buy {
                println!("buy order");

                // let (lowest_price, lowest_orders) = self.asks.first_key_value().unwrap();

                self.order_matching(order);

                //no order available at this price, create new vec with this order
            } else {
                //TODO: if bids in orderbook is empty throw error
                println!("sell order");

                self.order_matching(order);
            }
        }
    }

    fn cancel_order(&self, order_id: String) {
        todo!()
    }

    fn get_order(&self, order_id: String) {
        todo!()
    }

    fn get_depth(&self) {
        todo!()
    }

    fn limit_order_matching(&mut self, order: OpenOrder) {
        println!("order matching called");

        let mut new_order = Order::new(
            order.request_id.clone(),
            order.user_id.clone(),
            order.order_id.clone(),
            order.order_type.clone(),
            order.side.clone(),
            order.symbol.clone(),
            order.quantity,
            order.price,
            Utc::now().timestamp_millis() as u32,
            vec![],
        );

        let mut filled_quantity = 0;

        loop {
            let ord_key_val = if order.side == Buy {
                self.asks.first_key_value().unwrap().to_owned()
            } else {
                self.bids.last_key_value().unwrap().to_owned()
            };

            let book_order_price = ord_key_val.0.to_owned();

            let book_orders = ord_key_val.1.clone();

            println!(
                "book_orders: {:?}, book_order_price: {}",
                book_orders, book_order_price
            );

            let order_cond = if order.side == Buy {
                order.price < book_order_price
            } else {
                order.price > book_order_price
            };
            //order got matched, order will be fully or Partially filled
            if order_cond {
                //book doesn't have correct price for this order, return will fill quantity, push to
                //book with remainning quantity

                let ord_price = order.price;

                let new_order = OpenOrder {
                    request_id: order.request_id.clone(),
                    user_id: order.user_id.clone(),
                    order_id: order.order_id.clone(),
                    order_type: order.order_type.clone(),
                    side: order.side.clone(),
                    symbol: order.symbol.clone(),
                    quantity: order.quantity - filled_quantity,
                    price: order.price,
                    created_at: order.created_at,
                };

                // self.bids.insert(ord_price, order_arr);
                println!("new_order: {:?}", &new_order);

                if order.side == Buy {
                    self.bids
                        .entry(ord_price)
                        .or_default()
                        .push_front(new_order);

                    println!("filled_quantity: {}", filled_quantity);

                    //TODO: Return with fills and filled quantity, book has no longer match order for this price, push remainning quantity to the book
                    return;
                } else {
                    self.asks
                        .entry(ord_price)
                        .or_default()
                        .push_front(new_order);

                    println!("filled_quantity: {}", filled_quantity);

                    //TODO: Return with fills and filled quantity, book has no longer match order for this price, push remainning quantity to the book
                    return;
                }
            } else {
                for o in book_orders.iter() {
                    println!("looping over order: {:?}", o);

                    //equal to order then remove from book, if remainning less then just
                    //decrease from book
                    if o.quantity >= (order.quantity - filled_quantity) {
                        //order will be filled by this order only but this also could apply for
                        //second iteration

                        //TODO: generate fill id
                        let fill = Fill::new(
                            "fill_123".to_string(),
                            order.user_id.clone(),
                            order.order_id.clone(),
                            o.user_id.clone(),
                            o.order_id.clone(),
                            o.quantity,
                            order.order_type.clone(),
                            Buy.clone(),
                            order.symbol.clone(),
                            order.quantity,
                            book_order_price,
                            Utc::now().timestamp_millis() as u32,
                        );

                        new_order.fills.push(fill);

                        //TODO: increase the fill , decrease matched quantity
                        filled_quantity += order.quantity - filled_quantity;

                        //TODO:  push fill to fill array
                        // returing, remove from book

                        if order.side == Buy {
                            let ask_orders = self.asks.get_mut(&book_order_price).unwrap();

                            println!("ask_orders: {:?}:", ask_orders);

                            let index = ask_orders
                                .iter()
                                .position(|ord| ord.clone() == o.clone())
                                .unwrap();

                            println!("matched order index: {}", index);

                            if o.quantity > (order.quantity - filled_quantity) {
                                //decrease current order quantity
                                let current_ord = ask_orders.get_mut(index).unwrap();
                                current_ord.quantity -= (order.quantity - filled_quantity);
                            } else {
                                ask_orders.remove(index);
                            }

                            //TODO: order fully filled return with fill and filled_quantity
                            println!("order fully filled return");
                            return;
                        } else {
                            let bid_orders = self.bids.get_mut(&book_order_price).unwrap();

                            println!("bid_orders: {:?}:", bid_orders);

                            let index = bid_orders
                                .iter()
                                .position(|ord| ord.clone() == o.clone())
                                .unwrap();

                            println!("matched order index: {}", index);

                            if o.quantity > (order.quantity - filled_quantity) {
                                //decrease current order quantity
                                let current_ord = bid_orders.get_mut(index).unwrap();
                                current_ord.quantity -= (order.quantity - filled_quantity);
                            } else {
                                bid_orders.remove(index);
                            }

                            //TODO: order fully filled return with fill and filled_quantity
                            println!("order fully filled return");
                            return;
                        }
                    } else {
                        //order will not filled by this only, just increase the fill and push to
                        //fills array , remote current order from book
                        let fill = Fill::new(
                            "fill_456".to_string(),
                            order.user_id.clone(),
                            order.order_id.clone(),
                            o.user_id.clone(),
                            o.order_id.clone(),
                            o.quantity,
                            order.order_type.clone(),
                            Buy.clone(),
                            order.symbol.clone(),
                            order.quantity,
                            book_order_price,
                            Utc::now().timestamp_millis() as u32,
                        );

                        new_order.fills.push(fill);

                        filled_quantity += o.quantity;

                        //TODO:  push fill to fill array
                        //remove from book

                        if order.side == Buy {
                            let ask_orders = self.asks.get_mut(&book_order_price).unwrap();

                            println!("ask_orders: {:?}:", ask_orders);

                            let index = ask_orders
                                .iter()
                                .position(|ord| ord.clone() == o.clone())
                                .unwrap();

                            println!("matched order index: {}", index);

                            ask_orders.remove(index);
                        } else {
                            let bid_orders = self.bids.get_mut(&book_order_price).unwrap();

                            println!("bid_orders: {:?}:", bid_orders);

                            let index = bid_orders
                                .iter()
                                .position(|ord| ord.clone() == o.clone())
                                .unwrap();

                            println!("matched order index: {}", index);

                            bid_orders.remove(index);
                        }
                    }
                }
            }

            if order.side == Buy {
                let ask_ords = self.asks.get(&book_order_price).unwrap();

                if ask_ords.len() == 0 {
                    self.asks.remove(&book_order_price);
                }
            } else {
                let bid_ords = self.bids.get(&book_order_price).unwrap();

                if bid_ords.len() == 0 {
                    self.bids.remove(&book_order_price);
                }
            }
        }
    }
}
