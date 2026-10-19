# matching-engine-rust

A crypto spot and perpetual futures order matching engine written in Rust.

## Features

- Limit and market order matching (buy/sell)
- Order book with price-time priority (`BTreeMap` + `VecDeque`)
- Create, cancel, and get orders; fetch order book depth
- Self-trade prevention
- Forced liquidation support
- Redis pub/sub integration for backend, risk engine, and DB worker messaging

## Getting Started

```sh
cp .env.example .env   # set REDIS_URL
cargo run
```

Configuration is read from a `.env` file via `dotenvy`:

```sh
REDIS_URL=rediss://default:<password>@<host>:6379
```

## Project Structure

- `src/orderbook.rs` — core types (`Side`, `OrderType`, `Order`, `OpenOrder`, `Fill`, `ORDERBOOK`)
- `src/order.rs` — matching logic (`Orderbook` trait: create/cancel/get order, depth, limit & market matching)
- `src/main.rs` — Redis service layer, engine loop, and command handling
- `src/test.rs` — tests
