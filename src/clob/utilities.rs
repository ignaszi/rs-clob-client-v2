//! Client-side utility functions for orderbook analysis, fee calculation, and price validation.

use std::fmt::Write as _;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sha1::Digest as _;

use super::types::response::{OrderBookSummaryResponse, OrderSummary};
use super::types::{OrderType, Side, TickSize};

/// Walks orderbook levels in reverse (worst-to-best), accumulating via `accumulate`,
/// and returns the cutoff price where cumulative ≥ `target`.
///
/// If no level satisfies the target:
/// - Returns `None` for [`OrderType::FOK`]
/// - Returns the best available price (first level) for other order types
///
/// Returns `None` for empty `levels`.
pub(crate) fn walk_levels<F: Fn(&OrderSummary) -> Decimal>(
    levels: &[OrderSummary],
    target: Decimal,
    accumulate: F,
    order_type: &OrderType,
) -> Option<Decimal> {
    if levels.is_empty() {
        return None;
    }

    let mut total = Decimal::ZERO;
    for level in levels.iter().rev() {
        total += accumulate(level);
        if total >= target {
            return Some(level.price);
        }
    }

    if *order_type == OrderType::FOK {
        return None;
    }

    Some(levels[0].price)
}

/// Walks the orderbook to calculate the effective fill price for a given amount.
///
/// For BUY, walks asks and accumulates cumulative USDC cost (`size * price`).
/// For SELL, walks bids and accumulates cumulative token size.
/// Returns `None` for [`OrderType::FOK`] if insufficient liquidity,
/// or the best available price for other order types.
#[must_use]
pub fn calculate_market_price(
    orderbook: &OrderBookSummaryResponse,
    side: Side,
    amount: Decimal,
    order_type: &OrderType,
) -> Option<Decimal> {
    match side {
        Side::Buy => walk_levels(&orderbook.asks, amount, |l| l.size * l.price, order_type),
        Side::Sell => walk_levels(&orderbook.bids, amount, |l| l.size, order_type),
        Side::Unknown => None,
    }
}

/// Generates a server-compatible SHA1 hash of an orderbook snapshot.
///
/// Constructs a compact JSON payload with a specific key order
/// (`market`, `asset_id`, `timestamp`, `hash=""`, `bids`, `asks`,
/// `min_order_size`, `tick_size`, `neg_risk`, `last_trade_price`)
/// and returns the SHA1 hex digest.
///
/// **Note**: [`OrderBookSummaryResponse::hash()`] uses SHA-256 on `serde_json::to_string`
/// and produces different results. This function is for server-compatible verification.
#[must_use]
pub fn orderbook_summary_hash(orderbook: &OrderBookSummaryResponse) -> String {
    // Build JSON manually — serde_json::json! uses BTreeMap which sorts keys alphabetically,
    // but the server expects a specific non-alphabetical key order.
    let mut json = String::with_capacity(512);

    json.push('{');
    let _ = write!(json, "\"market\":\"{}\"", orderbook.market);

    let asset_id_json = serde_json::to_string(&orderbook.asset_id).unwrap_or_default();
    let _ = write!(json, ",\"asset_id\":{asset_id_json}");
    let _ = write!(
        json,
        ",\"timestamp\":\"{}\"",
        orderbook.timestamp.timestamp_millis()
    );
    json.push_str(",\"hash\":\"\"");

    json.push_str(",\"bids\":[");
    for (i, o) in orderbook.bids.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let _ = write!(
            json,
            "{{\"price\":\"{}\",\"size\":\"{}\"}}",
            o.price, o.size
        );
    }
    json.push(']');

    json.push_str(",\"asks\":[");
    for (i, o) in orderbook.asks.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let _ = write!(
            json,
            "{{\"price\":\"{}\",\"size\":\"{}\"}}",
            o.price, o.size
        );
    }
    json.push(']');

    let _ = write!(json, ",\"min_order_size\":\"{}\"", orderbook.min_order_size);
    let _ = write!(
        json,
        ",\"tick_size\":\"{}\"",
        Decimal::from(orderbook.tick_size)
    );
    let _ = write!(json, ",\"neg_risk\":{}", orderbook.neg_risk);
    let last = orderbook.last_trade_price.unwrap_or(Decimal::ZERO);
    let _ = write!(json, ",\"last_trade_price\":\"{last}\"");
    json.push('}');

    let mut hasher = sha1::Sha1::new();
    hasher.update(json.as_bytes());
    let result = hasher.finalize();

    format!("{result:x}")
}

/// Adjusts a market buy USDC amount to account for platform and builder fees.
///
/// Only adjusts when `user_usdc_balance <= total_cost`. Returns the effective
/// amount that can be traded after fees, or the original amount if balance is sufficient.
#[must_use]
pub fn adjust_market_buy_amount(
    amount: Decimal,
    user_usdc_balance: Decimal,
    price: Decimal,
    fee_rate: Decimal,
    fee_exponent: Decimal,
    builder_taker_fee_rate: Decimal,
) -> Decimal {
    let base = price * (Decimal::ONE - price);
    let base_f64: f64 = base.try_into().unwrap_or(0.0);
    let exp_f64: f64 = fee_exponent.try_into().unwrap_or(0.0);
    let platform_fee_rate =
        fee_rate * Decimal::try_from(base_f64.powf(exp_f64)).unwrap_or(Decimal::ZERO);

    let platform_fee = amount / price * platform_fee_rate;
    let total_cost = amount + platform_fee + amount * builder_taker_fee_rate;

    if user_usdc_balance < total_cost {
        let divisor = Decimal::ONE + platform_fee_rate / price + builder_taker_fee_rate;
        user_usdc_balance / divisor
    } else {
        amount
    }
}

/// Validates that a price is within the valid range `[tick_size, 1 - tick_size]`.
#[must_use]
pub fn price_valid(price: Decimal, tick_size: TickSize) -> bool {
    let ts = Decimal::from(tick_size);
    price >= ts && price <= dec!(1) - ts
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use rust_decimal_macros::dec;

    use super::*;
    use crate::types::{B256, U256};

    fn make_orderbook(
        bids: Vec<OrderSummary>,
        asks: Vec<OrderSummary>,
    ) -> OrderBookSummaryResponse {
        OrderBookSummaryResponse::builder()
            .market(B256::ZERO)
            .asset_id(U256::ZERO)
            .timestamp(Utc::now())
            .bids(bids)
            .asks(asks)
            .min_order_size(dec!(0.01))
            .neg_risk(false)
            .tick_size(TickSize::Hundredth)
            .build()
    }

    fn order(price: Decimal, size: Decimal) -> OrderSummary {
        OrderSummary::builder().price(price).size(size).build()
    }

    #[test]
    fn calculate_market_price_buy_sufficient_liquidity() {
        let ob = make_orderbook(
            vec![],
            vec![
                order(dec!(0.50), dec!(100)),
                order(dec!(0.51), dec!(100)),
                order(dec!(0.52), dec!(100)),
            ],
        );
        // Reversed walk: 0.52*100=52, 0.51*100=51, total=103 >= 80
        let result = calculate_market_price(&ob, Side::Buy, dec!(80), &OrderType::FOK);
        assert_eq!(result, Some(dec!(0.51)));
    }

    #[test]
    fn calculate_market_price_buy_insufficient_fok() {
        let ob = make_orderbook(vec![], vec![order(dec!(0.50), dec!(10))]);
        let result = calculate_market_price(&ob, Side::Buy, dec!(100), &OrderType::FOK);
        assert_eq!(result, None);
    }

    #[test]
    fn calculate_market_price_buy_insufficient_fak() {
        let ob = make_orderbook(
            vec![],
            vec![order(dec!(0.50), dec!(10)), order(dec!(0.60), dec!(5))],
        );
        let result = calculate_market_price(&ob, Side::Buy, dec!(1000), &OrderType::FAK);
        assert_eq!(result, Some(dec!(0.50)));
    }

    #[test]
    fn calculate_market_price_sell() {
        let ob = make_orderbook(
            vec![
                order(dec!(0.50), dec!(100)),
                order(dec!(0.49), dec!(100)),
                order(dec!(0.48), dec!(100)),
            ],
            vec![],
        );
        // Reversed walk: 0.48 (100), 0.49 (200), need 150 tokens
        let result = calculate_market_price(&ob, Side::Sell, dec!(150), &OrderType::FOK);
        assert_eq!(result, Some(dec!(0.49)));
    }

    #[test]
    fn calculate_market_price_empty_orderbook() {
        let ob = make_orderbook(vec![], vec![]);
        assert_eq!(
            calculate_market_price(&ob, Side::Buy, dec!(100), &OrderType::FOK),
            None,
        );
    }

    #[test]
    fn calculate_market_price_unknown_side_returns_none() {
        let ob = make_orderbook(
            vec![order(dec!(0.49), dec!(100))],
            vec![order(dec!(0.51), dec!(100))],
        );
        assert_eq!(
            calculate_market_price(&ob, Side::Unknown, dec!(10), &OrderType::FOK),
            None,
        );
    }

    #[test]
    fn price_valid_within_bounds() {
        assert!(price_valid(dec!(0.5), TickSize::Hundredth));
        assert!(price_valid(dec!(0.01), TickSize::Hundredth));
        assert!(price_valid(dec!(0.99), TickSize::Hundredth));
    }

    #[test]
    fn price_valid_at_boundaries() {
        assert!(price_valid(dec!(0.1), TickSize::Tenth));
        assert!(price_valid(dec!(0.9), TickSize::Tenth));
    }

    #[test]
    fn price_valid_out_of_bounds() {
        assert!(!price_valid(dec!(0.0), TickSize::Hundredth));
        assert!(!price_valid(dec!(1.0), TickSize::Hundredth));
        assert!(!price_valid(dec!(0.005), TickSize::Hundredth));
        assert!(!price_valid(dec!(0.995), TickSize::Hundredth));
    }

    #[test]
    fn price_valid_all_tick_sizes() {
        assert!(price_valid(dec!(0.5), TickSize::Tenth));
        assert!(price_valid(dec!(0.5), TickSize::Hundredth));
        assert!(price_valid(dec!(0.5), TickSize::Thousandth));
        assert!(price_valid(dec!(0.5), TickSize::TenThousandth));
    }

    #[test]
    fn orderbook_hash_deterministic() {
        let ts = DateTime::from_timestamp_millis(1_700_000_000_000).expect("valid ts");
        let ob = OrderBookSummaryResponse::builder()
            .market(B256::ZERO)
            .asset_id(U256::ZERO)
            .timestamp(ts)
            .bids(vec![order(dec!(0.49), dec!(50))])
            .asks(vec![order(dec!(0.51), dec!(25))])
            .min_order_size(dec!(0.01))
            .neg_risk(false)
            .tick_size(TickSize::Hundredth)
            .build();

        let hash = orderbook_summary_hash(&ob);
        assert_eq!(hash.len(), 40);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(hash, orderbook_summary_hash(&ob));
    }

    #[test]
    fn orderbook_hash_differs_for_different_inputs() {
        let ts = DateTime::from_timestamp_millis(1_700_000_000_000).expect("valid ts");
        let ob1 = OrderBookSummaryResponse::builder()
            .market(B256::ZERO)
            .asset_id(U256::from(1_u64))
            .timestamp(ts)
            .min_order_size(dec!(0.01))
            .neg_risk(false)
            .tick_size(TickSize::Hundredth)
            .build();

        let ob2 = OrderBookSummaryResponse::builder()
            .market(B256::ZERO)
            .asset_id(U256::from(2_u64))
            .timestamp(ts)
            .min_order_size(dec!(0.01))
            .neg_risk(false)
            .tick_size(TickSize::Hundredth)
            .build();

        assert_ne!(orderbook_summary_hash(&ob1), orderbook_summary_hash(&ob2));
    }

    #[test]
    fn adjust_market_buy_no_adjustment_when_balance_sufficient() {
        let result = adjust_market_buy_amount(
            dec!(100),
            dec!(1000),
            dec!(0.5),
            dec!(0.02),
            dec!(1),
            dec!(0),
        );
        assert_eq!(result, dec!(100));
    }

    #[test]
    fn adjust_market_buy_adjusts_when_balance_insufficient() {
        let result = adjust_market_buy_amount(
            dec!(100),
            dec!(100),
            dec!(0.5),
            dec!(0.02),
            dec!(1),
            dec!(0),
        );
        assert!(result < dec!(100));
        assert!(result > dec!(0));
    }

    #[test]
    fn adjust_market_buy_with_builder_fee() {
        let result = adjust_market_buy_amount(
            dec!(100),
            dec!(100),
            dec!(0.5),
            dec!(0),
            dec!(1),
            dec!(0.005),
        );
        // effective * 1.005 = 100
        let expected = dec!(100) / dec!(1.005);
        assert_eq!(result, expected);
    }
}
