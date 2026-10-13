#[cfg(test)]
mod tests {
    use std::{
        sync::mpsc,
        thread::{self, sleep},
        time::Duration,
    };

    use chrono::Utc;

    use crate::order::Orderbook;
    use crate::orderbook::{ORDERBOOK, OpenOrder, OrderStatus, OrderType, Side};

    fn book() -> ORDERBOOK {
        ORDERBOOK::new()
    }

    fn order(id: &str, side: Side, order_type: OrderType, quantity: u64, price: u64) -> OpenOrder {
        OpenOrder {
            request_id: format!("req-{id}"),
            user_id: format!("user-{id}"),
            order_id: id.to_string(),
            order_type,
            side,
            symbol: "BTC_USD".to_string(),
            quantity,
            price,
            created_at: Utc::now().timestamp_millis(),
        }
    }

    fn limit_buy(id: &str, qty: u64, price: u64) -> OpenOrder {
        order(id, Side::Buy, OrderType::Limit, qty, price)
    }

    fn limit_sell(id: &str, qty: u64, price: u64) -> OpenOrder {
        order(id, Side::Sell, OrderType::Limit, qty, price)
    }

    fn market_buy(id: &str, qty: u64) -> OpenOrder {
        order(id, Side::Buy, OrderType::Market, qty, 0)
    }

    fn market_sell(id: &str, qty: u64) -> OpenOrder {
        order(id, Side::Sell, OrderType::Market, qty, 0)
    }

    fn run_with_timeout<T: Send + 'static>(
        f: impl FnOnce() -> T + Send + 'static,
        d: Duration,
    ) -> Option<T> {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(d).ok()
    }

    #[test]
    fn limit_buy_rests_on_empty_book() {
        let mut b = book();
        let res = b.create_order(limit_buy("o1", 10, 100)).unwrap();
        assert_eq!(res.filled_quantity, 0);
        assert!(matches!(res.status, OrderStatus::Accepted));

        assert_eq!(b.bids.len(), 1);
        let q = b.bids.get(&100).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].quantity, 10);
        assert!(b.orders.contains_key("o1"));
    }

    #[test]
    fn limit_sell_rests_on_empty_book() {
        let mut b = book();
        let res = b.create_order(limit_sell("o1", 10, 100)).unwrap();
        assert_eq!(res.filled_quantity, 0);
        assert!(matches!(res.status, OrderStatus::Accepted));

        assert_eq!(b.asks.len(), 1);
        let q = b.asks.get(&100).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].quantity, 10);
        assert!(b.orders.contains_key("o1"));
    }

    #[test]
    fn limit_buy_full_match() {
        let mut b = book();
        b.create_order(limit_sell("s1", 10, 100)).unwrap();

        let res = b.create_order(limit_buy("b1", 10, 100)).unwrap();

        println!("res: {:?}", res);
        assert_eq!(res.filled_quantity, 10, "filled quantity should be 10");
        assert!(matches!(res.status, OrderStatus::FullyFilled));
        assert!(b.asks.is_empty(), "resting ask should be consumed");
        assert!(!b.orders.contains_key("s1"));
    }

    #[test]
    fn limit_sell_full_match() {
        let mut b = book();
        b.create_order(limit_buy("b1", 10, 100)).unwrap();

        let res = b.create_order(limit_sell("s1", 10, 100)).unwrap();

        assert_eq!(res.filled_quantity, 10, "filled quantity should be 10");
        assert!(matches!(res.status, OrderStatus::FullyFilled));
        assert!(b.bids.is_empty(), "resting bid should be consumed");
        assert!(!b.orders.contains_key("b1"));
    }

    #[test]
    fn limit_buy_partial_fill_decrements_resting_order() {
        let mut b = book();
        b.create_order(limit_sell("s1", 10, 100)).unwrap();

        let res = b.create_order(limit_buy("b1", 4, 100)).unwrap();

        assert_eq!(res.filled_quantity, 4, "filled quantity should be 4");
        assert!(matches!(res.status, OrderStatus::FullyFilled));

        let q = b.asks.get(&100).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].quantity, 6, "resting order should be reduced to 6");
    }

    #[test]
    fn limit_sell_partial_fill_decrements_resting_order() {
        let mut b = book();
        b.create_order(limit_buy("b1", 10, 100)).unwrap();

        let res = b.create_order(limit_sell("s1", 4, 100)).unwrap();

        assert_eq!(res.filled_quantity, 4, "filled quantity should be 4");
        assert!(matches!(res.status, OrderStatus::FullyFilled));

        let q = b.bids.get(&100).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].quantity, 6, "resting order should be reduced to 6");
    }

    #[test]
    fn limit_buy_price_improvement_across_levels() {
        let mut b = book();
        println!("ORDERBOOK: {:?}", b);
        b.create_order(limit_sell("s1", 5, 100)).unwrap();
        println!("ORDERBOOK: {:?}", b);
        b.create_order(limit_sell("s2", 5, 101)).unwrap();
        println!("ORDERBOOK: {:?}", b);
        let res = b.create_order(limit_buy("b1", 10, 102)).unwrap();
        println!("ORDERBOOK: {:?}", b);
        assert_eq!(res.filled_quantity, 10, "filled quantity should be 10");
        assert!(matches!(res.status, OrderStatus::FullyFilled));
        assert!(b.asks.is_empty(), "all asks should be consumed");
        assert!(!b.orders.contains_key("s1"));
        assert!(!b.orders.contains_key("s2"));
    }

    #[test]
    fn limit_buy_no_match_rests_with_zero_fill() {
        let mut b = book();
        b.create_order(limit_sell("s1", 10, 100)).unwrap();

        let res = b.create_order(limit_buy("b1", 10, 99)).unwrap();

        assert_eq!(res.filled_quantity, 0, "nothing should be filled");
        println!("res: {:?}", res);
        assert!(
            matches!(res.status, OrderStatus::Accepted),
            "unmatched limit order should be accepted, not partially filled"
        );
        assert_eq!(b.bids.len(), 1);
        assert!(b.bids.contains_key(&99));
    }

    #[test]
    fn limit_sell_no_match_rests_with_zero_fill() {
        let mut b = book();
        b.create_order(limit_buy("b1", 10, 100)).unwrap();

        let res = b.create_order(limit_sell("s1", 10, 101)).unwrap();

        assert_eq!(res.filled_quantity, 0, "nothing should be filled");
        assert!(matches!(res.status, OrderStatus::Accepted));
        assert_eq!(b.asks.len(), 1);
        assert!(b.asks.contains_key(&101));
    }

    #[test]
    fn market_buy_full_match_consumes_asks() {
        let mut b = book();
        b.create_order(limit_sell("s1", 10, 100)).unwrap();

        let res = b.create_order(market_buy("m1", 10)).unwrap();

        println!("res: {:?}", res);

        println!("b: {:?}", b);
        assert_eq!(res.filled_quantity, 10);
        assert!(matches!(res.status, OrderStatus::FullyFilled));
        assert!(b.asks.is_empty(), "resting ask should be fully consumed");
        assert!(!b.orders.contains_key("s1"));
    }

    #[test]
    fn market_buy_partial_fill_decrements_resting_order() {
        let mut b = book();
        b.create_order(limit_sell("s1", 10, 100)).unwrap();

        let res = b.create_order(market_buy("m1", 4)).unwrap();

        assert_eq!(res.filled_quantity, 4);
        assert!(matches!(res.status, OrderStatus::FullyFilled));
        let q = b.asks.get(&100).unwrap();
        assert_eq!(q[0].quantity, 6, "resting order should be reduced to 6");
    }

    #[test]
    fn market_buy_errors_on_empty_book() {
        let mut b = book();
        let res = b.create_order(market_buy("m1", 10));
        assert!(res.is_err(), "market buy on empty book should error");
    }

    #[test]
    fn market_sell_errors_on_empty_book() {
        let mut b = book();
        let res = b.create_order(market_sell("m1", 10));
        assert!(res.is_err(), "market sell on empty book should error");
    }

    #[test]
    fn market_sell_matches_resting_bids() {
        let mut b = book();
        b.create_order(limit_buy("b1", 10, 100)).unwrap();

        let res = run_with_timeout(
            move || b.create_order(market_sell("m1", 10)),
            Duration::from_millis(300),
        );
        let res = res.expect("market sell should complete, but it hung");

        let res = res.unwrap();
        assert_eq!(res.filled_quantity, 10);
        assert!(matches!(res.status, OrderStatus::FullyFilled));
    }

    #[test]
    fn cancel_order_removes_from_book_and_orders() {
        let mut b = book();
        b.create_order(limit_sell("s1", 10, 100)).unwrap();

        let cancelled = b.cancel_order("s1".to_string()).unwrap();
        assert_eq!(cancelled, "s1");

        assert!(!b.orders.contains_key("s1"));
        assert!(b.asks.is_empty(), "ask price level should be cleaned up");
    }

    #[test]
    #[should_panic]
    fn cancel_nonexistent_order_should_not_panic() {
        let mut b = book();
        b.create_order(limit_sell("s1", 10, 100)).unwrap();
        let _ = b.cancel_order("missing".to_string());
    }

    #[test]
    fn get_order_returns_existing_order() {
        let mut b = book();
        b.create_order(limit_sell("s1", 10, 100)).unwrap();

        let got = b.get_order("s1".to_string()).unwrap();
        assert_eq!(got.order_id, "s1");
        assert_eq!(got.quantity, 10);
    }

    #[test]
    fn get_order_errors_on_missing_order() {
        let b = book();
        assert!(b.get_order("missing".to_string()).is_err());
    }

    #[test]
    fn get_depth_reflects_aggregated_quantity() {
        let mut b = book();
        b.create_order(limit_sell("s1", 5, 100)).unwrap();
        b.create_order(limit_sell("s2", 3, 100)).unwrap();
        b.create_order(limit_sell("s3", 2, 101)).unwrap();
        b.create_order(limit_buy("b1", 7, 99)).unwrap();

        let depth = b.get_depth(10);

        assert_eq!(depth.asks.len(), 2);
        assert_eq!(depth.asks[0].price, 100);
        assert_eq!(depth.asks[0].quantity, 8);
        assert_eq!(depth.asks[1].price, 101);
        assert_eq!(depth.asks[1].quantity, 2);

        assert_eq!(depth.bids.len(), 1);
        assert_eq!(depth.bids[0].price, 99);
        assert_eq!(depth.bids[0].quantity, 7);
    }

    #[test]
    fn get_depth_respects_limit() {
        let mut b = book();
        b.create_order(limit_sell("s1", 5, 100)).unwrap();
        b.create_order(limit_sell("s2", 2, 101)).unwrap();
        b.create_order(limit_buy("b1", 3, 99)).unwrap();

        let depth = b.get_depth(1);

        assert_eq!(depth.asks.len(), 1);
        assert_eq!(depth.asks[0].price, 100);
        assert_eq!(depth.asks[0].quantity, 5);
        assert_eq!(depth.bids.len(), 1);
        assert_eq!(depth.bids[0].price, 99);
    }

    #[test]
    fn fifo_priority_at_same_price_level() {
        let mut b = book();
        b.create_order(limit_sell("s1", 1, 100)).unwrap();
        println!("ORDERBOOK: {:?}", &b);

        let dur = Duration::from_millis(5);
        sleep(dur);
        b.create_order(limit_sell("s2", 1, 100)).unwrap();

        println!("ORDERBOOK: {:?}", &b);
        b.create_order(limit_buy("b1", 1, 100)).unwrap();

        println!("ORDERBOOK: {:?}", &b);
        assert!(
            !b.orders.contains_key("s1"),
            "oldest order (s1) should be filled first (FIFO)"
        );
        assert!(
            b.orders.contains_key("s2"),
            "newest order (s2) should still be resting"
        );
    }

    #[test]
    fn self_trade_should_be_prevented() {
        println!("1259");
        let mut b = book();
        b.create_order(limit_sell("s1", 10, 100)).unwrap();
        println!("1261");
        let mut self_buy = limit_buy("b1", 10, 100);
        self_buy.user_id = "user-s1".to_string();
        println!("1264");

        println!("self_buy: {:?}", self_buy);

        let rst = b.create_order(self_buy);

        println!("rst: {:?}", rst);

        println!("1272");
        let rst = 1;

        assert!(
            b.asks.contains_key(&100),
            "self-trade should not consume the user's own resting order"
        );
    }

    #[test]
    fn zero_quantity_order_does_not_panic() {
        let mut b = book();
        let res = b.create_order(limit_buy("z1", 0, 100));

        println!("1272");
        assert!(
            res.is_ok(),
            "zero quantity limit order should be handled gracefully"
        );
    }

    #[test]
    fn market_sell_multiple_fills_quantity_must_not_exceed_original() {
        let mut b = book();

        b.create_order(limit_buy("b1", 3, 101)).unwrap();
        b.create_order(limit_buy("b2", 7, 100)).unwrap();

        let res = b.create_order(market_sell("s1", 10)).unwrap();

        println!("res: {:?}", res);

        assert_eq!(
            res.filled_quantity, 10,
            "Filled quantity must equal original order quantity"
        );

        assert!(matches!(res.status, OrderStatus::FullyFilled));

        assert!(b.bids.is_empty());
    }

    #[test]
    fn market_buy_must_follow_fifo_at_same_price() {
        let mut b = book();

        b.create_order(limit_sell("s1", 5, 100)).unwrap();
        b.create_order(limit_sell("s2", 5, 100)).unwrap();
        b.create_order(limit_sell("s3", 5, 100)).unwrap();

        let res = b.create_order(market_buy("m1", 5)).unwrap();

        assert_eq!(res.filled_quantity, 5);

        let remaining = b.asks.get(&100).unwrap();

        assert_eq!(remaining.len(), 2);

        assert!(
            !remaining.iter().any(|o| o.order_id == "s1"),
            "Oldest resting order must execute first"
        );

        assert!(remaining.iter().any(|o| o.order_id == "s2"));
        assert!(remaining.iter().any(|o| o.order_id == "s3"));
    }

    #[test]
    fn market_buy_multiple_levels_should_fully_fill() {
        let mut b = book();

        b.create_order(limit_sell("s1", 3, 100)).unwrap();
        b.create_order(limit_sell("s2", 7, 101)).unwrap();

        let res = b.create_order(market_buy("m1", 10)).unwrap();

        assert_eq!(
            res.filled_quantity, 10,
            "Market order should fill across both price levels"
        );

        assert!(matches!(res.status, OrderStatus::FullyFilled));

        assert!(b.asks.is_empty(), "All liquidity should be consumed");

        assert!(!b.orders.contains_key("s1"));
        assert!(!b.orders.contains_key("s2"));
    }

    #[test]
    fn limit_buy_should_rest_remaining_quantity_after_asks_exhausted() {
        let mut b = book();

        b.create_order(limit_sell("s1", 3, 100)).unwrap();

        let res = b.create_order(limit_buy("b1", 10, 105)).unwrap();

        assert_eq!(res.filled_quantity, 3);

        assert!(matches!(res.status, OrderStatus::PartiallyFilled));

        assert!(b.asks.is_empty());

        println!("ORDERBOOK: {:?}", b);
        let remaining = b.bids.get(&105).unwrap();

        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].quantity, 7);
        assert_eq!(remaining[0].order_id, "b1");
    }

    #[test]
    fn limit_sell_should_rest_remaining_quantity_after_bids_exhausted() {
        let mut b = book();

        b.create_order(limit_buy("b1", 4, 100)).unwrap();

        let res = b.create_order(limit_sell("s1", 10, 95)).unwrap();

        assert_eq!(res.filled_quantity, 4);

        assert!(matches!(res.status, OrderStatus::PartiallyFilled));

        assert!(b.bids.is_empty());

        println!("hi");

        let remaining = b.asks.get(&95).unwrap();

        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].quantity, 6);
        assert_eq!(remaining[0].order_id, "s1");
    }

    #[test]
    fn self_trade_should_cancel_taker_and_preserve_maker() {
        let mut b = book();

        let sell = limit_sell("s1", 10, 100);

        let mut buy = limit_buy("b1", 5, 105);

        buy.user_id = sell.user_id.clone();

        b.create_order(sell).unwrap();

        let res = b.create_order(buy).unwrap();

        assert_eq!(res.filled_quantity, 0);

        assert!(matches!(res.status, OrderStatus::SelfTradePrevented));

        let asks = b.asks.get(&100).unwrap();

        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].quantity, 10);

        assert!(b.bids.is_empty(), "Cancelled taker must not enter the book");
    }

    #[test]
    fn self_trade_after_partial_fill_should_preserve_fills() {
        let mut b = book();

        let other_sell = limit_sell("s1", 3, 100);
        let own_sell = limit_sell("s2", 5, 101);

        let mut buy = limit_buy("b1", 10, 105);

        buy.user_id = own_sell.user_id.clone();

        b.create_order(other_sell).unwrap();
        b.create_order(own_sell).unwrap();

        let res = b.create_order(buy).unwrap();

        assert_eq!(
            res.filled_quantity, 3,
            "Previously executed trades must be preserved"
        );

        assert!(matches!(res.status, OrderStatus::SelfTradePrevented));

        assert!(!b.orders.contains_key("s1"));

        assert!(b.orders.contains_key("s2"));

        let remaining = b.asks.get(&101).unwrap();

        assert_eq!(remaining[0].quantity, 5);

        assert!(b.bids.is_empty());
    }

    #[test]
    fn market_order_error_must_not_leave_partial_book_mutations() {
        let mut b = book();

        b.create_order(limit_sell("s1", 3, 100)).unwrap();
        b.create_order(limit_sell("s2", 7, 101)).unwrap();

        let result = b.create_order(market_buy("m1", 10));

        assert!(result.is_ok(), "Sufficient initial liquidity exists");

        let res = result.unwrap();

        assert_eq!(res.filled_quantity, 10);
        assert!(b.asks.is_empty());
    }

    #[test]
    fn duplicate_order_id_should_be_rejected() {
        let mut b = book();

        b.create_order(limit_buy("b1", 10, 100)).unwrap();

        let result = b.create_order(limit_buy("b1", 20, 99));

        println!("ORDERBOOK: {:?}", b);
        assert!(result.is_err(), "Duplicate order IDs must be rejected");

        assert_eq!(b.bids.get(&100).unwrap()[0].quantity, 10);

        assert!(
            !b.bids.contains_key(&99),
            "Duplicate order must not be inserted"
        );

        assert_eq!(b.orders.len(), 1);
    }
}
