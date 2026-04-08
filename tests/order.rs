#![cfg(feature = "clob")]
#![allow(
    clippy::unwrap_used,
    reason = "Do not need additional syntax for setting up tests, and https://github.com/rust-lang/rust-clippy/issues/13981"
)]

mod common;

use std::str::FromStr as _;

use alloy::primitives::{B256, U256};
use chrono::{DateTime, Utc};
use httpmock::MockServer;
use polymarket_client_sdk_v2::clob::types::response::OrderSummary;
use polymarket_client_sdk_v2::clob::types::{Amount, OrderType, Side, SignatureType, TickSize};
use polymarket_client_sdk_v2::types::{Address, Decimal, address};
use reqwest::StatusCode;
use rust_decimal_macros::dec;

use crate::common::{
    USDC_DECIMALS, create_authenticated, ensure_requirements, to_decimal, token_1, token_2,
};

/// Tests for the lifecycle of a [`Client`] as it moves from [`Unauthenticated`] to [`Authenticated`]
mod lifecycle {
    use alloy::signers::Signer as _;
    use alloy::signers::local::LocalSigner;
    use polymarket_client_sdk_v2::POLYGON;
    use polymarket_client_sdk_v2::clob::{Client, Config};
    use polymarket_client_sdk_v2::error::Validation;
    use serde_json::json;

    use super::*;
    use crate::common::{API_KEY, PASSPHRASE, POLY_ADDRESS, PRIVATE_KEY, SECRET};

    #[tokio::test]
    async fn client_order_fields_should_persist_new_order() -> anyhow::Result<()> {
        let server = MockServer::start();
        let signer = LocalSigner::from_str(PRIVATE_KEY)?.with_chain_id(Some(POLYGON));

        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/auth/derive-api-key")
                .header(POLY_ADDRESS, signer.address().to_string().to_lowercase());
            then.status(StatusCode::OK).json_body(json!({
                "apiKey": API_KEY.to_string(),
                "passphrase": PASSPHRASE,
                "secret": SECRET
            }));
        });

        let client = Client::new(&server.base_url(), Config::default())?
            .authentication_builder(&signer)
            .salt_generator(|| 1)
            .authenticate()
            .await?;

        ensure_requirements(&server, token_1(), TickSize::Tenth);
        ensure_requirements(&server, token_2(), TickSize::Thousandth);

        let signable_order = client
            .limit_order()
            .token_id(token_1())
            .size(Decimal::ONE_HUNDRED)
            .price(dec!(0.1))
            .side(Side::Buy)
            .build()
            .await?;

        let signable_order_2 = client
            .limit_order()
            .token_id(token_2())
            .price(dec!(0.512))
            .size(Decimal::ONE_HUNDRED)
            .side(Side::Buy)
            .build()
            .await?;

        assert_eq!(signable_order.order().salt, U256::from(1));
        assert_eq!(signable_order_2.order().salt, U256::from(1));
        assert_ne!(signable_order, signable_order_2);
        mock.assert();

        Ok(())
    }

    #[tokio::test]
    async fn client_order_fields_should_reset_on_deauthenticate() -> anyhow::Result<()> {
        let server = MockServer::start();
        let signer = LocalSigner::from_str(PRIVATE_KEY)?.with_chain_id(Some(POLYGON));

        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/auth/derive-api-key")
                .header(POLY_ADDRESS, signer.address().to_string().to_lowercase());
            then.status(StatusCode::OK).json_body(json!({
                "apiKey": API_KEY.to_string(),
                "passphrase": PASSPHRASE,
                "secret": SECRET
            }));
        });

        let client = Client::new(&server.base_url(), Config::default())?
            .authentication_builder(&signer)
            .salt_generator(|| 1)
            .funder(address!("0xd1615A7B6146cDbA40a559eC876A3bcca4050890"))
            .signature_type(SignatureType::GnosisSafe)
            .authenticate()
            .await?;

        ensure_requirements(&server, token_1(), TickSize::Tenth);

        let signable_order = client
            .limit_order()
            .token_id(token_1())
            .size(Decimal::ONE_HUNDRED)
            .price(dec!(0.1))
            .side(Side::Buy)
            .build()
            .await?;

        assert_eq!(signable_order.order().salt, U256::from(1));
        assert_eq!(
            signable_order.order().signatureType,
            SignatureType::GnosisSafe as u8
        );

        let client = client
            .deauthenticate()
            .await?
            .authentication_builder(&signer)
            .salt_generator(|| 123)
            .authenticate()
            .await?;

        let signable_order = client
            .limit_order()
            .token_id(token_1())
            .size(Decimal::ONE_HUNDRED)
            .price(dec!(0.1))
            .side(Side::Buy)
            .build()
            .await?;

        assert_eq!(signable_order.order().salt, U256::from(123));
        assert_eq!(
            signable_order.order().signatureType,
            SignatureType::Eoa as u8
        );
        assert_eq!(signable_order.order().maker, signer.address());

        mock.assert_calls(2);

        Ok(())
    }

    #[tokio::test]
    async fn client_with_funder_should_succeed() -> anyhow::Result<()> {
        let server = MockServer::start();

        let signer = LocalSigner::from_str(PRIVATE_KEY)?.with_chain_id(Some(POLYGON));
        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/auth/derive-api-key")
                .header(POLY_ADDRESS, signer.address().to_string().to_lowercase());
            then.status(StatusCode::OK).json_body(json!({
                "apiKey": API_KEY.to_string(),
                "passphrase": PASSPHRASE,
                "secret": SECRET
            }));
        });

        let funder = address!("0xaDEFf2158d668f64308C62ef227C5CcaCAAf976D");
        let client = Client::new(&server.base_url(), Config::default())?
            .authentication_builder(&signer)
            .funder(funder)
            .signature_type(SignatureType::Proxy)
            .authenticate()
            .await?;

        mock.assert();

        ensure_requirements(&server, token_1(), TickSize::Tenth);

        let signable_order = client
            .limit_order()
            .token_id(token_1())
            .size(Decimal::ONE_HUNDRED)
            .price(dec!(0.1))
            .side(Side::Buy)
            .build()
            .await?;

        assert_eq!(signable_order.order().maker, funder);
        assert_eq!(
            signable_order.order().signatureType,
            SignatureType::Proxy as u8
        );

        assert_eq!(signable_order.order().side, Side::Buy as u8);
        assert_ne!(signable_order.order().maker, signable_order.order().signer);

        ensure_requirements(&server, token_2(), TickSize::Tenth);

        let signable_order = client
            .limit_order()
            .token_id(token_2())
            .size(Decimal::TEN)
            .price(dec!(0.2))
            .side(Side::Sell)
            .build()
            .await?;

        // Funder and signature type propagate from setting on the auth builder
        assert_eq!(signable_order.order().maker, funder);
        assert_eq!(
            signable_order.order().signatureType,
            SignatureType::Proxy as u8
        );

        assert_eq!(signable_order.order().side, Side::Sell as u8);
        assert_ne!(signable_order.order().maker, signable_order.order().signer);

        Ok(())
    }

    #[tokio::test]
    async fn client_logged_in_then_out_should_reset_funder_and_signature_type() -> anyhow::Result<()>
    {
        let server = MockServer::start();

        let signer = LocalSigner::from_str(PRIVATE_KEY)?.with_chain_id(Some(POLYGON));
        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/auth/derive-api-key")
                .header(POLY_ADDRESS, signer.address().to_string().to_lowercase());
            then.status(StatusCode::OK).json_body(json!({
                "apiKey": API_KEY.to_string(),
                "passphrase": PASSPHRASE,
                "secret": SECRET
            }));
        });

        let funder = address!("0xaDEFf2158d668f64308C62ef227C5CcaCAAf976D");
        let client = Client::new(&server.base_url(), Config::default())?
            .authentication_builder(&signer)
            .funder(funder)
            .signature_type(SignatureType::Proxy)
            .authenticate()
            .await?;

        mock.assert();

        ensure_requirements(&server, token_1(), TickSize::Tenth);

        let signable_order = client
            .limit_order()
            .token_id(token_1())
            .size(Decimal::ONE_HUNDRED)
            .price(dec!(0.1))
            .side(Side::Buy)
            .build()
            .await?;

        assert_eq!(signable_order.order().maker, funder);
        assert_eq!(
            signable_order.order().signatureType,
            SignatureType::Proxy as u8
        );

        assert_eq!(signable_order.order().side, Side::Buy as u8);
        assert_ne!(signable_order.order().maker, signable_order.order().signer);

        ensure_requirements(&server, token_2(), TickSize::Tenth);

        client.deauthenticate().await?;
        let client = Client::new(&server.base_url(), Config::default())?
            .authentication_builder(&signer)
            .authenticate()
            .await?;

        let signable_order = client
            .limit_order()
            .token_id(token_2())
            .size(Decimal::TEN)
            .price(dec!(0.2))
            .side(Side::Sell)
            .build()
            .await?;

        // Funder and signature type propagate from setting on the auth builder
        assert_eq!(signable_order.order().maker, signer.address());
        assert_eq!(
            signable_order.order().signatureType,
            SignatureType::Eoa as u8
        );

        assert_eq!(signable_order.order().side, Side::Sell as u8);
        assert_eq!(signable_order.order().maker, signable_order.order().signer);

        Ok(())
    }

    #[tokio::test]
    async fn incompatible_funder_and_signature_types_should_fail() -> anyhow::Result<()> {
        let server = MockServer::start();

        let signer = LocalSigner::from_str(PRIVATE_KEY)?.with_chain_id(Some(POLYGON));

        let funder = address!("0xaDEFf2158d668f64308C62ef227C5CcaCAAf976D");
        let err = Client::new(&server.base_url(), Config::default())?
            .authentication_builder(&signer)
            .funder(funder)
            .signature_type(SignatureType::Eoa)
            .authenticate()
            .await
            .unwrap_err();
        let msg = &err.downcast_ref::<Validation>().unwrap().reason;

        assert_eq!(
            msg,
            "Cannot have a funder address with a Eoa signature type"
        );

        // Note: Using GnosisSafe without explicit funder now auto-derives from signer.address()
        // So this case now succeeds - tested in funder_auto_derived_from_signer_for_proxy_types

        let err = Client::new(&server.base_url(), Config::default())?
            .authentication_builder(&signer)
            .funder(Address::ZERO)
            .signature_type(SignatureType::GnosisSafe)
            .authenticate()
            .await
            .unwrap_err();
        let msg = &err.downcast_ref::<Validation>().unwrap().reason;

        assert_eq!(
            msg,
            "Cannot have a zero funder address with a GnosisSafe signature type"
        );

        Ok(())
    }

    /// Tests that the funder address is automatically derived using CREATE2 from
    /// the signer's EOA when using Proxy or `GnosisSafe` signature types without
    /// explicit funder.
    #[tokio::test]
    async fn funder_auto_derived_from_signer_for_proxy_types() -> anyhow::Result<()> {
        use polymarket_client_sdk_v2::{POLYGON, derive_proxy_wallet, derive_safe_wallet};

        let server = MockServer::start();
        let signer = LocalSigner::from_str(PRIVATE_KEY)?.with_chain_id(Some(POLYGON));

        // Expected CREATE2-derived addresses for this signer
        let expected_safe_addr =
            derive_safe_wallet(signer.address(), POLYGON).expect("Safe derivation failed");
        let expected_proxy_addr =
            derive_proxy_wallet(signer.address(), POLYGON).expect("Proxy derivation failed");

        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/auth/derive-api-key")
                .header(POLY_ADDRESS, signer.address().to_string().to_lowercase());
            then.status(StatusCode::OK).json_body(json!({
                "apiKey": API_KEY.to_string(),
                "passphrase": PASSPHRASE,
                "secret": SECRET
            }));
        });

        // GnosisSafe without explicit funder - should auto-derive using CREATE2
        let client = Client::new(&server.base_url(), Config::default())?
            .authentication_builder(&signer)
            .signature_type(SignatureType::GnosisSafe)
            .authenticate()
            .await?;

        ensure_requirements(&server, token_1(), TickSize::Tenth);

        let signable_order = client
            .limit_order()
            .token_id(token_1())
            .size(Decimal::ONE_HUNDRED)
            .price(dec!(0.5))
            .side(Side::Buy)
            .build()
            .await?;

        // Verify maker (funder) is the CREATE2-derived Safe address
        assert_eq!(signable_order.order().maker, expected_safe_addr);
        // Signer remains the EOA
        assert_eq!(signable_order.order().signer, signer.address());
        // Maker and signer should be different for proxy types
        assert_ne!(signable_order.order().maker, signable_order.order().signer);
        assert_eq!(
            signable_order.order().signatureType,
            SignatureType::GnosisSafe as u8
        );

        // Now test with SignatureType::Proxy
        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/auth/derive-api-key")
                .header(POLY_ADDRESS, signer.address().to_string().to_lowercase());
            then.status(StatusCode::OK).json_body(json!({
                "apiKey": API_KEY.to_string(),
                "passphrase": PASSPHRASE,
                "secret": SECRET
            }));
        });

        let client = Client::new(&server.base_url(), Config::default())?
            .authentication_builder(&signer)
            .signature_type(SignatureType::Proxy)
            .authenticate()
            .await?;

        ensure_requirements(&server, token_1(), TickSize::Tenth);

        let signable_order = client
            .limit_order()
            .token_id(token_1())
            .size(Decimal::ONE_HUNDRED)
            .price(dec!(0.5))
            .side(Side::Buy)
            .build()
            .await?;

        // Verify maker (funder) is the CREATE2-derived Proxy address
        assert_eq!(signable_order.order().maker, expected_proxy_addr);
        // Signer remains the EOA
        assert_eq!(signable_order.order().signer, signer.address());
        // Maker and signer should be different for proxy types
        assert_ne!(signable_order.order().maker, signable_order.order().signer);
        assert_eq!(
            signable_order.order().signatureType,
            SignatureType::Proxy as u8
        );

        Ok(())
    }

    /// Tests that explicit funder address overrides the auto-derivation.
    #[tokio::test]
    async fn explicit_funder_overrides_auto_derivation() -> anyhow::Result<()> {
        let server = MockServer::start();
        let signer = LocalSigner::from_str(PRIVATE_KEY)?.with_chain_id(Some(POLYGON));
        let explicit_funder = address!("0xaDEFf2158d668f64308C62ef227C5CcaCAAf976D");

        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/auth/derive-api-key")
                .header(POLY_ADDRESS, signer.address().to_string().to_lowercase());
            then.status(StatusCode::OK).json_body(json!({
                "apiKey": API_KEY.to_string(),
                "passphrase": PASSPHRASE,
                "secret": SECRET
            }));
        });

        // GnosisSafe with explicit funder - should use the explicit one
        let client = Client::new(&server.base_url(), Config::default())?
            .authentication_builder(&signer)
            .funder(explicit_funder)
            .signature_type(SignatureType::GnosisSafe)
            .authenticate()
            .await?;

        ensure_requirements(&server, token_1(), TickSize::Tenth);

        let signable_order = client
            .limit_order()
            .token_id(token_1())
            .size(Decimal::ONE_HUNDRED)
            .price(dec!(0.5))
            .side(Side::Buy)
            .build()
            .await?;

        // Verify maker (funder) is the explicitly provided one, not auto-derived
        assert_eq!(signable_order.order().maker, explicit_funder);
        assert_eq!(signable_order.order().signer, signer.address());
        assert_ne!(signable_order.order().maker, signable_order.order().signer);
        assert_eq!(
            signable_order.order().signatureType,
            SignatureType::GnosisSafe as u8
        );

        Ok(())
    }

    #[tokio::test]
    async fn signer_with_no_chain_id_should_fail() -> anyhow::Result<()> {
        let server = MockServer::start();

        let signer = LocalSigner::from_str(PRIVATE_KEY)?;

        let err = Client::new(&server.base_url(), Config::default())?
            .authentication_builder(&signer)
            .authenticate()
            .await
            .unwrap_err();
        let msg = &err.downcast_ref::<Validation>().unwrap().reason;

        assert_eq!(
            msg,
            "Chain id not set, be sure to provide one on the signer"
        );

        Ok(())
    }

    #[tokio::test]
    async fn signer_with_unsupported_chain_id_should_fail() -> anyhow::Result<()> {
        let server = MockServer::start();

        let signer = LocalSigner::from_str(PRIVATE_KEY)?.with_chain_id(Some(1));

        let err = Client::new(&server.base_url(), Config::default())?
            .authentication_builder(&signer)
            .authenticate()
            .await
            .unwrap_err();
        let msg = &err.downcast_ref::<Validation>().unwrap().reason;

        assert_eq!(msg, "Only Polygon and AMOY are supported, got 1");

        Ok(())
    }
}

mod limit {
    use polymarket_client_sdk_v2::error::Validation;

    use super::*;

    #[tokio::test]
    async fn should_fail_on_expiration_for_gtc() -> anyhow::Result<()> {
        let server = MockServer::start();
        let client = create_authenticated(&server).await?;

        ensure_requirements(&server, token_1(), TickSize::Tenth);

        let err = client
            .limit_order()
            .token_id(token_1())
            .price(dec!(0.5))
            .size(dec!(21.04))
            .side(Side::Buy)
            .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
            .build()
            .await
            .unwrap_err();
        let msg = &err.downcast_ref::<Validation>().unwrap().reason;

        assert_eq!(msg, "Only GTD orders may have a non-zero expiration");

        Ok(())
    }

    #[tokio::test]
    async fn should_fail_on_post_only_for_non_gtc_gtd() -> anyhow::Result<()> {
        let server = MockServer::start();
        let client = create_authenticated(&server).await?;

        ensure_requirements(&server, token_1(), TickSize::Tenth);

        let err = client
            .limit_order()
            .token_id(token_1())
            .price(dec!(0.5))
            .size(dec!(21.04))
            .side(Side::Buy)
            .order_type(OrderType::FOK)
            .post_only(true)
            .build()
            .await
            .unwrap_err();
        let msg = &err.downcast_ref::<Validation>().unwrap().reason;

        assert_eq!(msg, "postOnly is only supported for GTC and GTD orders");

        Ok(())
    }

    #[tokio::test]
    async fn should_fail_on_missing_fields() -> anyhow::Result<()> {
        let server = MockServer::start();
        let client = create_authenticated(&server).await?;

        ensure_requirements(&server, token_1(), TickSize::Tenth);

        let err = client
            .limit_order()
            .token_id(token_1())
            .size(dec!(21.04))
            .side(Side::Buy)
            .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
            .build()
            .await
            .unwrap_err();
        let msg = &err.downcast_ref::<Validation>().unwrap().reason;

        assert_eq!(msg, "Unable to build Order due to missing price");

        let err = client
            .limit_order()
            .token_id(token_1())
            .price(dec!(0.5))
            .side(Side::Buy)
            .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
            .build()
            .await
            .unwrap_err();
        let msg = &err.downcast_ref::<Validation>().unwrap().reason;

        assert_eq!(msg, "Unable to build Order due to missing size");

        Ok(())
    }

    #[tokio::test]
    async fn should_fail_on_too_granular_of_a_price() -> anyhow::Result<()> {
        let server = MockServer::start();
        let client = create_authenticated(&server).await?;

        ensure_requirements(&server, token_1(), TickSize::Hundredth);

        let err = client
            .limit_order()
            .token_id(token_1())
            .price(dec!(0.005))
            .size(dec!(21.04))
            .side(Side::Buy)
            .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
            .build()
            .await
            .unwrap_err();
        let msg = &err.downcast_ref::<Validation>().unwrap().reason;

        assert_eq!(
            msg,
            "Unable to build Order: Price 0.005 has 3 decimal places. Minimum tick size 0.01 has 2 decimal places. Price decimal places <= minimum tick size decimal places"
        );

        Ok(())
    }

    #[tokio::test]
    async fn should_fail_on_negative_price_and_size() -> anyhow::Result<()> {
        let server = MockServer::start();
        let client = create_authenticated(&server).await?;

        ensure_requirements(&server, token_1(), TickSize::Tenth);

        let err = client
            .limit_order()
            .token_id(token_1())
            .price(dec!(-0.5))
            .size(dec!(21.04))
            .side(Side::Buy)
            .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
            .build()
            .await
            .unwrap_err();
        let msg = &err.downcast_ref::<Validation>().unwrap().reason;

        assert_eq!(msg, "Unable to build Order due to negative price -0.5");

        let err = client
            .limit_order()
            .token_id(token_1())
            .price(dec!(0.5))
            .size(dec!(-21.04))
            .side(Side::Buy)
            .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
            .build()
            .await
            .unwrap_err();
        let msg = &err.downcast_ref::<Validation>().unwrap().reason;

        assert_eq!(msg, "Unable to build Order due to negative size -21.04");

        Ok(())
    }

    mod buy {
        use super::*;

        #[tokio::test]
        async fn should_succeed_0_1() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Tenth);

            let signable_order = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.5))
                .size(dec!(21.04))
                .side(Side::Buy)
                .order_type(OrderType::GTD)
                .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
                .build()
                .await?;

            let maker_amount = signable_order.order().makerAmount;
            let taker_amount = signable_order.order().takerAmount;

            let price = to_decimal(maker_amount) / to_decimal(taker_amount);
            assert_eq!(price, dec!(0.50));

            assert_eq!(signable_order.order().maker, client.address());
            assert_eq!(signable_order.order().signer, client.address());

            assert_eq!(signable_order.order().tokenId, token_1());
            assert_eq!(signable_order.order().makerAmount, U256::from(10_520_000));
            assert_eq!(signable_order.order().takerAmount, U256::from(21_040_000));
            assert_eq!(signable_order.payload.expiration, U256::from(50000));

            assert_eq!(signable_order.order().side, Side::Buy as u8);
            assert_eq!(
                signable_order.order().signatureType,
                SignatureType::Eoa as u8
            );

            Ok(())
        }

        #[tokio::test]
        async fn should_succeed_0_01() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Hundredth);

            let signable_order = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.56))
                .size(dec!(21.04))
                .side(Side::Buy)
                .order_type(OrderType::GTD)
                .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
                .build()
                .await?;

            let maker_amount = signable_order.order().makerAmount;
            let taker_amount = signable_order.order().takerAmount;

            let price = to_decimal(maker_amount) / to_decimal(taker_amount);
            assert_eq!(price, dec!(0.56));

            assert_eq!(signable_order.order().maker, client.address());
            assert_eq!(signable_order.order().signer, client.address());

            assert_eq!(signable_order.order().tokenId, token_1());
            assert_eq!(signable_order.order().makerAmount, U256::from(11_782_400));
            assert_eq!(signable_order.order().takerAmount, U256::from(21_040_000));
            assert_eq!(signable_order.payload.expiration, U256::from(50000));

            assert_eq!(signable_order.order().side, Side::Buy as u8);
            assert_eq!(
                signable_order.order().signatureType,
                SignatureType::Eoa as u8
            );

            Ok(())
        }

        #[tokio::test]
        async fn should_succeed_0_001() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Thousandth);

            let signable_order = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.056))
                .size(dec!(21.04))
                .side(Side::Buy)
                .order_type(OrderType::GTD)
                .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
                .build()
                .await?;

            let maker_amount = signable_order.order().makerAmount;
            let taker_amount = signable_order.order().takerAmount;

            let price = to_decimal(maker_amount) / to_decimal(taker_amount);
            assert_eq!(price, dec!(0.056));

            assert_eq!(signable_order.order().maker, client.address());
            assert_eq!(signable_order.order().signer, client.address());

            assert_eq!(signable_order.order().tokenId, token_1());
            assert_eq!(signable_order.order().makerAmount, U256::from(1_178_240));
            assert_eq!(signable_order.order().takerAmount, U256::from(21_040_000));
            assert_eq!(signable_order.payload.expiration, U256::from(50000));

            assert_eq!(signable_order.order().side, Side::Buy as u8);
            assert_eq!(
                signable_order.order().signatureType,
                SignatureType::Eoa as u8
            );

            Ok(())
        }

        #[tokio::test]
        async fn should_succeed_0_0001() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::TenThousandth);

            let signable_order = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.0056))
                .size(dec!(21.04))
                .side(Side::Buy)
                .order_type(OrderType::GTD)
                .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
                .build()
                .await?;

            let maker_amount = signable_order.order().makerAmount;
            let taker_amount = signable_order.order().takerAmount;

            let price = to_decimal(maker_amount) / to_decimal(taker_amount);
            assert_eq!(price, dec!(0.0056));

            assert_eq!(signable_order.order().maker, client.address());
            assert_eq!(signable_order.order().signer, client.address());

            assert_eq!(signable_order.order().tokenId, token_1());
            assert_eq!(signable_order.order().makerAmount, U256::from(117_824));
            assert_eq!(signable_order.order().takerAmount, U256::from(21_040_000));
            assert_eq!(signable_order.payload.expiration, U256::from(50000));

            assert_eq!(signable_order.order().side, Side::Buy as u8);
            assert_eq!(
                signable_order.order().signatureType,
                SignatureType::Eoa as u8
            );

            Ok(())
        }

        #[tokio::test]
        async fn buy_should_succeed_decimal_accuracy() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Hundredth);

            let signable_order = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.24))
                .size(dec!(15))
                .side(Side::Buy)
                .build()
                .await?;

            assert_eq!(signable_order.order().makerAmount, U256::from(3_600_000));
            assert_eq!(signable_order.order().takerAmount, U256::from(15_000_000));

            Ok(())
        }

        #[tokio::test]
        async fn buy_should_succeed_decimal_accuracy_2() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Hundredth);

            let signable_order = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.82))
                .size(dec!(101))
                .side(Side::Buy)
                .build()
                .await?;

            assert_eq!(signable_order.order().makerAmount, U256::from(82_820_000));
            assert_eq!(signable_order.order().takerAmount, U256::from(101_000_000));

            Ok(())
        }

        #[tokio::test]
        async fn buy_should_fail_on_too_granular_of_lot_size() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Hundredth);

            let err = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.78))
                .size(dec!(12.8205))
                .side(Side::Buy)
                .build()
                .await
                .unwrap_err();
            let validation_err = err.downcast_ref::<Validation>().unwrap();

            assert_eq!(
                validation_err.reason,
                "Unable to build Order: Size 12.8205 has 4 decimal places. Maximum lot size is 2"
            );

            Ok(())
        }

        #[tokio::test]
        async fn buy_should_succeed_decimal_accuracy_4() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Hundredth);

            let signable_order = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.58))
                .size(dec!(18233.33))
                .side(Side::Buy)
                .build()
                .await?;

            assert_eq!(
                signable_order.order().makerAmount,
                U256::from(10_575_331_400_u64)
            );
            assert_eq!(
                signable_order.order().takerAmount,
                U256::from(18_233_330_000_u64)
            );

            Ok(())
        }
    }

    mod sell {
        use super::*;

        #[tokio::test]
        async fn should_succeed_0_1() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Tenth);

            let signable_order = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.5))
                .size(dec!(21.04))
                .side(Side::Sell)
                .order_type(OrderType::GTD)
                .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
                .build()
                .await?;

            let maker_amount = signable_order.order().makerAmount;
            let taker_amount = signable_order.order().takerAmount;

            let price = to_decimal(taker_amount) / to_decimal(maker_amount);
            assert_eq!(price, dec!(0.50));

            assert_eq!(signable_order.order().maker, client.address());
            assert_eq!(signable_order.order().signer, client.address());

            assert_eq!(signable_order.order().tokenId, token_1());
            assert_eq!(signable_order.order().makerAmount, U256::from(21_040_000));
            assert_eq!(signable_order.order().takerAmount, U256::from(10_520_000));
            assert_eq!(signable_order.payload.expiration, U256::from(50000));

            assert_eq!(signable_order.order().side, Side::Sell as u8);
            assert_eq!(
                signable_order.order().signatureType,
                SignatureType::Eoa as u8
            );

            Ok(())
        }

        #[tokio::test]
        async fn should_succeed_0_01() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Hundredth);

            let signable_order = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.56))
                .size(dec!(21.04))
                .side(Side::Sell)
                .order_type(OrderType::GTD)
                .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
                .build()
                .await?;

            let maker_amount = signable_order.order().makerAmount;
            let taker_amount = signable_order.order().takerAmount;

            let price = to_decimal(taker_amount) / to_decimal(maker_amount);
            assert_eq!(price, dec!(0.56));

            assert_eq!(signable_order.order().maker, client.address());
            assert_eq!(signable_order.order().signer, client.address());

            assert_eq!(signable_order.order().tokenId, token_1());
            assert_eq!(signable_order.order().makerAmount, U256::from(21_040_000));
            assert_eq!(signable_order.order().takerAmount, U256::from(11_782_400));
            assert_eq!(signable_order.payload.expiration, U256::from(50000));

            assert_eq!(signable_order.order().side, Side::Sell as u8);
            assert_eq!(
                signable_order.order().signatureType,
                SignatureType::Eoa as u8
            );

            Ok(())
        }

        #[tokio::test]
        async fn should_succeed_0_001() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Thousandth);

            let signable_order = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.056))
                .size(dec!(21.04))
                .side(Side::Sell)
                .order_type(OrderType::GTD)
                .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
                .build()
                .await?;

            let maker_amount = signable_order.order().makerAmount;
            let taker_amount = signable_order.order().takerAmount;

            let price = to_decimal(taker_amount) / to_decimal(maker_amount);
            assert_eq!(price, dec!(0.056));

            assert_eq!(signable_order.order().maker, client.address());
            assert_eq!(signable_order.order().signer, client.address());

            assert_eq!(signable_order.order().tokenId, token_1());
            assert_eq!(signable_order.order().makerAmount, U256::from(21_040_000));
            assert_eq!(signable_order.order().takerAmount, U256::from(1_178_240));
            assert_eq!(signable_order.payload.expiration, U256::from(50000));

            assert_eq!(signable_order.order().side, Side::Sell as u8);
            assert_eq!(
                signable_order.order().signatureType,
                SignatureType::Eoa as u8
            );

            Ok(())
        }

        #[tokio::test]
        async fn should_succeed_0_0001() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::TenThousandth);

            let signable_order = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.0056))
                .size(dec!(21.04))
                .side(Side::Sell)
                .order_type(OrderType::GTD)
                .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
                .build()
                .await?;

            let maker_amount = signable_order.order().makerAmount;
            let taker_amount = signable_order.order().takerAmount;

            let price = to_decimal(taker_amount) / to_decimal(maker_amount);
            assert_eq!(price, dec!(0.0056));

            assert_eq!(signable_order.order().maker, client.address());
            assert_eq!(signable_order.order().signer, client.address());

            assert_eq!(signable_order.order().tokenId, token_1());
            assert_eq!(signable_order.order().makerAmount, U256::from(21_040_000));
            assert_eq!(signable_order.order().takerAmount, U256::from(117_824));
            assert_eq!(signable_order.payload.expiration, U256::from(50000));

            assert_eq!(signable_order.order().side, Side::Sell as u8);
            assert_eq!(
                signable_order.order().signatureType,
                SignatureType::Eoa as u8
            );

            Ok(())
        }

        #[tokio::test]
        async fn sell_should_succeed_decimal_accuracy() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Hundredth);

            let signable_order = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.24))
                .size(dec!(15))
                .side(Side::Sell)
                .build()
                .await?;

            assert_eq!(signable_order.order().makerAmount, U256::from(15_000_000));
            assert_eq!(signable_order.order().takerAmount, U256::from(3_600_000));

            Ok(())
        }

        #[tokio::test]
        async fn sell_should_succeed_decimal_accuracy_2() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Hundredth);

            let signable_order = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.82))
                .size(dec!(101))
                .side(Side::Sell)
                .build()
                .await?;

            assert_eq!(signable_order.order().makerAmount, U256::from(101_000_000));
            assert_eq!(signable_order.order().takerAmount, U256::from(82_820_000));

            Ok(())
        }

        #[tokio::test]
        async fn sell_should_succeed_decimal_accuracy_3() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Hundredth);

            let err = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.78))
                .size(dec!(12.8205))
                .side(Side::Sell)
                .build()
                .await
                .unwrap_err();

            let validation_err = err.downcast_ref::<Validation>().unwrap();

            assert_eq!(
                validation_err.reason,
                "Unable to build Order: Size 12.8205 has 4 decimal places. Maximum lot size is 2"
            );

            Ok(())
        }

        #[tokio::test]
        async fn sell_should_succeed_decimal_accuracy_4() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Hundredth);

            let signable_order = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.39))
                .size(dec!(2435.89))
                .side(Side::Sell)
                .build()
                .await?;

            assert_eq!(
                signable_order.order().makerAmount,
                U256::from(2_435_890_000_u64)
            );
            assert_eq!(signable_order.order().takerAmount, U256::from(949_997_100));

            Ok(())
        }

        #[tokio::test]
        async fn sell_should_succeed_decimal_accuracy_5() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Hundredth);

            let signable_order = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.43))
                .size(dec!(19.1))
                .side(Side::Sell)
                .build()
                .await?;

            assert_eq!(signable_order.order().makerAmount, U256::from(19_100_000));
            assert_eq!(signable_order.order().takerAmount, U256::from(8_213_000));

            Ok(())
        }
    }

    #[tokio::test]
    async fn should_succeed() -> anyhow::Result<()> {
        let server = MockServer::start();
        let client = create_authenticated(&server).await?;

        ensure_requirements(&server, token_1(), TickSize::Thousandth);
        ensure_requirements(&server, token_2(), TickSize::Hundredth);

        assert_eq!(
            client.tick_size(token_1()).await?.minimum_tick_size,
            TickSize::Thousandth
        );

        let signable_order = client
            .limit_order()
            .token_id(token_1())
            .price(dec!(0.512))
            .size(Decimal::ONE_HUNDRED)
            .side(Side::Buy)
            .build()
            .await?;

        assert_eq!(signable_order.order().maker, client.address());

        assert_eq!(signable_order.order().tokenId, token_1());
        assert_eq!(signable_order.order().makerAmount, U256::from(51_200_000));
        assert_eq!(signable_order.order().takerAmount, U256::from(100_000_000));
        assert_eq!(signable_order.payload.expiration, U256::ZERO);

        assert_eq!(signable_order.order().side, Side::Buy as u8);
        assert_eq!(
            signable_order.order().signatureType,
            SignatureType::Eoa as u8
        );

        let signable_order = client
            .limit_order()
            .token_id(token_2())
            .price(dec!(0.78))
            .size(dec!(12.82))
            .side(Side::Buy)
            .build()
            .await?;

        assert_eq!(signable_order.order().maker, client.address());

        assert_eq!(signable_order.order().tokenId, token_2());
        assert_eq!(signable_order.order().makerAmount, U256::from(9_999_600));
        assert_eq!(signable_order.order().takerAmount, U256::from(12_820_000));
        assert_eq!(signable_order.payload.expiration, U256::ZERO);

        assert_eq!(signable_order.order().side, Side::Buy as u8);
        assert_eq!(
            signable_order.order().signatureType,
            SignatureType::Eoa as u8
        );

        let _order = client
            .limit_order()
            .token_id(token_2())
            .order_type(OrderType::GTC)
            .price(dec!(0.78))
            .size(dec!(12.82))
            .side(Side::Sell)
            .build()
            .await?;

        Ok(())
    }
}

mod market {
    use polymarket_client_sdk_v2::error::Validation;
    use serde_json::json;

    use super::*;

    fn ensure_requirements_for_market_price(
        server: &MockServer,
        token_id: U256,
        bids: &[OrderSummary],
        asks: &[OrderSummary],
    ) {
        let minimum_tick_size = TickSize::Tenth;

        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/book")
                .query_param("token_id", token_id.to_string());
            then.status(StatusCode::OK).json_body(json!({
                "market": "0xbd31dc8a20211944f6b70f31557f1001557b59905b7738480ca09bd4532f84af",
                "asset_id": token_id,
                "timestamp": "1000",
                "bids": bids,
                "asks": asks,
                "min_order_size": "5",
                "neg_risk": false,
                "tick_size": minimum_tick_size.as_decimal(),
            }));
        });

        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/tick-size")
                .query_param("token_id", token_id.to_string());
            then.status(StatusCode::OK).json_body(json!({
                "minimum_tick_size": minimum_tick_size.as_decimal(),
            }));
        });

        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/fee-rate")
                .query_param("token_id", token_id.to_string());
            then.status(StatusCode::OK)
                .json_body(json!({ "base_fee": 0 }));
        });
    }

    mod buy {
        use super::*;

        mod fok {
            use polymarket_client_sdk_v2::error::Validation;

            use super::*;

            #[tokio::test]
            async fn should_fail_on_no_asks() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(&server, token_1(), &[], &[]);

                let err = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                    .side(Side::Buy)
                    .order_type(OrderType::FOK)
                    .build()
                    .await
                    .unwrap_err();
                let msg = &err.downcast_ref::<Validation>().unwrap().reason;

                assert_eq!(
                    msg,
                    "No opposing orders for 15871154585880608648532107628464183779895785213830018178010423617714102767076 which means there is no market price"
                );

                Ok(())
            }

            #[tokio::test]
            async fn should_fail_on_insufficient_liquidity() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[],
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                    ],
                );

                let err = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                    .side(Side::Buy)
                    .order_type(OrderType::FOK)
                    .build()
                    .await
                    .unwrap_err();
                let msg = &err.downcast_ref::<Validation>().unwrap().reason;

                assert_eq!(
                    msg,
                    "Insufficient liquidity to fill order for 15871154585880608648532107628464183779895785213830018178010423617714102767076 at 100"
                );

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[],
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                    ],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                    .side(Side::Buy)
                    .order_type(OrderType::FOK)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(maker_amount) / to_decimal(taker_amount);
                assert_eq!(price, dec!(0.5));

                assert_eq!(signable_order.order().maker, client.address());
                assert_eq!(signable_order.order().signer, client.address());

                assert_eq!(
                    signable_order.order().tokenId,
                    U256::from_str(
                        "15871154585880608648532107628464183779895785213830018178010423617714102767076"
                    )?
                );
                assert_eq!(signable_order.order().makerAmount, U256::from(100_000_000)); // 100 USDC
                assert_eq!(signable_order.order().takerAmount, U256::from(200_000_000)); // 200 `token_1()` tokens
                assert_eq!(signable_order.payload.expiration, U256::ZERO);

                assert_eq!(signable_order.order().side, Side::Buy as u8);
                assert_eq!(
                    signable_order.order().signatureType,
                    SignatureType::Eoa as u8
                );

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed2() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[],
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(dec!(200))
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                    ],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                    .side(Side::Buy)
                    .order_type(OrderType::FOK)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(maker_amount) / to_decimal(taker_amount);
                assert_eq!(price, dec!(0.4));

                assert_eq!(maker_amount, U256::from(100_000_000)); // 100 USDC
                assert_eq!(taker_amount, U256::from(250_000_000)); // 250 `token_1()` tokens

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed_3() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[],
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(dec!(120))
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.2))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                    ],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                    .side(Side::Buy)
                    .order_type(OrderType::FOK)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(maker_amount) / to_decimal(taker_amount);
                assert_eq!(price, dec!(0.5));

                assert_eq!(maker_amount, U256::from(100_000_000)); // 100 USDC
                assert_eq!(taker_amount, U256::from(200_000_000)); // 200 `token_1()` tokens

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed_4() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[],
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(dec!(200))
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                    ],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                    .side(Side::Buy)
                    .order_type(OrderType::FOK)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(maker_amount) / to_decimal(taker_amount);
                assert_eq!(price, dec!(0.5));

                assert_eq!(maker_amount, U256::from(100_000_000)); // 100 USDC
                assert_eq!(taker_amount, U256::from(200_000_000)); // 200 `token_1()` tokens

                Ok(())
            }
        }

        mod fak {
            use super::*;

            #[tokio::test]
            async fn should_fail_on_no_asks() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(&server, token_1(), &[], &[]);

                let err = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                    .side(Side::Buy)
                    .build()
                    .await
                    .unwrap_err();
                let msg = &err.downcast_ref::<Validation>().unwrap().reason;

                assert_eq!(
                    msg,
                    "No opposing orders for 15871154585880608648532107628464183779895785213830018178010423617714102767076 which means there is no market price"
                );

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[],
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                    ],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                    .side(Side::Buy)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(maker_amount) / to_decimal(taker_amount);
                assert_eq!(price, dec!(0.5));

                assert_eq!(signable_order.order().maker, client.address());
                assert_eq!(signable_order.order().signer, client.address());

                assert_eq!(
                    signable_order.order().tokenId,
                    U256::from_str(
                        "15871154585880608648532107628464183779895785213830018178010423617714102767076"
                    )?
                );
                assert_eq!(signable_order.order().makerAmount, U256::from(100_000_000)); // 100 USDC
                assert_eq!(signable_order.order().takerAmount, U256::from(200_000_000)); // 200 `token_1()` tokens
                assert_eq!(signable_order.payload.expiration, U256::ZERO);

                assert_eq!(signable_order.order().side, Side::Buy as u8);
                assert_eq!(
                    signable_order.order().signatureType,
                    SignatureType::Eoa as u8
                );

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed_2() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[],
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                    ],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                    .side(Side::Buy)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(maker_amount) / to_decimal(taker_amount);
                assert_eq!(price, dec!(0.5));

                assert_eq!(maker_amount, U256::from(100_000_000)); // 100 USDC
                assert_eq!(taker_amount, U256::from(200_000_000)); // 200 `token_1()` tokens

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed_3() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[],
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(dec!(200))
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                    ],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                    .side(Side::Buy)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(maker_amount) / to_decimal(taker_amount);
                assert_eq!(price, dec!(0.4));

                assert_eq!(maker_amount, U256::from(100_000_000)); // 100 USDC
                assert_eq!(taker_amount, U256::from(250_000_000)); // 250 `token_1()` tokens

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed_4() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[],
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(dec!(120))
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                    ],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                    .side(Side::Buy)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(maker_amount) / to_decimal(taker_amount);
                assert_eq!(price, dec!(0.5));

                assert_eq!(maker_amount, U256::from(100_000_000)); // 100 USDC
                assert_eq!(taker_amount, U256::from(200_000_000)); // 200 `token_1()` tokens

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed_5() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[],
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(dec!(200))
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                    ],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                    .side(Side::Buy)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(maker_amount) / to_decimal(taker_amount);
                assert_eq!(price, dec!(0.5));

                assert_eq!(maker_amount, U256::from(100_000_000)); // 100 USDC
                assert_eq!(taker_amount, U256::from(200_000_000)); // 200 `token_1()` tokens

                Ok(())
            }
        }

        #[tokio::test]
        async fn should_succeed_0_1() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Tenth);
            // Always gives a market price of 0.5 for 100
            ensure_requirements_for_market_price(
                &server,
                token_1(),
                &[],
                &[OrderSummary::builder()
                    .price(dec!(0.5))
                    .size(Decimal::ONE_HUNDRED)
                    .build()],
            );

            let signable_order = client
                .market_order()
                .token_id(token_1())
                .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                .side(Side::Buy)
                .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
                .build()
                .await?;

            let maker_amount = signable_order.order().makerAmount;
            let taker_amount = signable_order.order().takerAmount;

            let price = to_decimal(maker_amount) / to_decimal(taker_amount);
            assert_eq!(price, dec!(0.50));

            assert_eq!(signable_order.order().maker, client.address());
            assert_eq!(signable_order.order().signer, client.address());

            assert_eq!(signable_order.order().tokenId, token_1());
            assert_eq!(signable_order.order().makerAmount, U256::from(100_000_000));
            assert_eq!(signable_order.order().takerAmount, U256::from(200_000_000));
            assert_eq!(signable_order.payload.expiration, U256::from(0));

            assert_eq!(signable_order.order().side, Side::Buy as u8);
            assert_eq!(
                signable_order.order().signatureType,
                SignatureType::Eoa as u8
            );

            Ok(())
        }

        #[tokio::test]
        async fn should_succeed_0_01() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Hundredth);
            // Always gives a market price of 0.56 for 100
            ensure_requirements_for_market_price(
                &server,
                token_1(),
                &[],
                &[OrderSummary::builder()
                    .price(dec!(0.56))
                    .size(Decimal::ONE_HUNDRED)
                    .build()],
            );

            let signable_order = client
                .market_order()
                .token_id(token_1())
                .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                .side(Side::Buy)
                .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
                .build()
                .await?;

            let maker_amount = signable_order.order().makerAmount;
            let taker_amount = signable_order.order().takerAmount;

            let price = (to_decimal(maker_amount) / to_decimal(taker_amount))
                .trunc_with_scale(USDC_DECIMALS);
            assert_eq!(price, dec!(0.56));

            assert_eq!(signable_order.order().maker, client.address());
            assert_eq!(signable_order.order().signer, client.address());

            assert_eq!(signable_order.order().tokenId, token_1());
            assert_eq!(signable_order.order().makerAmount, U256::from(100_000_000));
            assert_eq!(signable_order.order().takerAmount, U256::from(178_571_400));
            assert_eq!(signable_order.payload.expiration, U256::from(0));

            assert_eq!(signable_order.order().side, Side::Buy as u8);
            assert_eq!(
                signable_order.order().signatureType,
                SignatureType::Eoa as u8
            );

            Ok(())
        }

        #[tokio::test]
        async fn should_succeed_0_001() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Thousandth);
            // Always gives a market price of 0.056 for 100
            ensure_requirements_for_market_price(
                &server,
                token_1(),
                &[],
                &[OrderSummary::builder()
                    .price(dec!(0.056))
                    .size(Decimal::ONE_HUNDRED)
                    .build()],
            );

            let signable_order = client
                .market_order()
                .token_id(token_1())
                .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                .side(Side::Buy)
                .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
                .build()
                .await?;

            let maker_amount = signable_order.order().makerAmount;
            let taker_amount = signable_order.order().takerAmount;

            let price = (to_decimal(maker_amount) / to_decimal(taker_amount))
                .trunc_with_scale(USDC_DECIMALS);
            assert_eq!(price, dec!(0.056));

            assert_eq!(signable_order.order().maker, client.address());
            assert_eq!(signable_order.order().signer, client.address());

            assert_eq!(signable_order.order().tokenId, token_1());
            assert_eq!(signable_order.order().makerAmount, U256::from(100_000_000));
            assert_eq!(
                signable_order.order().takerAmount,
                U256::from(1_785_714_280)
            );
            assert_eq!(signable_order.payload.expiration, U256::from(0));

            assert_eq!(signable_order.order().side, Side::Buy as u8);
            assert_eq!(
                signable_order.order().signatureType,
                SignatureType::Eoa as u8
            );

            Ok(())
        }

        #[tokio::test]
        async fn should_succeed_0_0001() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::TenThousandth);
            // Always gives a market price of 0.0056 for 100
            ensure_requirements_for_market_price(
                &server,
                token_1(),
                &[],
                &[OrderSummary::builder()
                    .price(dec!(0.0056))
                    .size(Decimal::ONE_HUNDRED)
                    .build()],
            );

            let signable_order = client
                .market_order()
                .token_id(token_1())
                .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                .side(Side::Buy)
                .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
                .build()
                .await?;

            let maker_amount = signable_order.order().makerAmount;
            let taker_amount = signable_order.order().takerAmount;

            let price = (to_decimal(maker_amount) / to_decimal(taker_amount))
                .trunc_with_scale(USDC_DECIMALS);
            assert_eq!(price, dec!(0.0056));

            assert_eq!(signable_order.order().maker, client.address());
            assert_eq!(signable_order.order().signer, client.address());

            assert_eq!(signable_order.order().tokenId, token_1());
            assert_eq!(signable_order.order().makerAmount, U256::from(100_000_000));
            assert_eq!(
                signable_order.order().takerAmount,
                U256::from(17_857_142_857_u64)
            );
            assert_eq!(signable_order.payload.expiration, U256::from(0));

            assert_eq!(signable_order.order().side, Side::Buy as u8);
            assert_eq!(
                signable_order.order().signatureType,
                SignatureType::Eoa as u8
            );

            Ok(())
        }

        #[tokio::test]
        async fn market_buy_with_shares_fok_should_fail_on_no_asks() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements_for_market_price(&server, token_1(), &[], &[]);

            let err = client
                .market_order()
                .token_id(token_1())
                .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
                .side(Side::Buy)
                .order_type(OrderType::FOK)
                .build()
                .await
                .unwrap_err();

            let msg = &err
                .downcast_ref::<polymarket_client_sdk_v2::error::Validation>()
                .unwrap()
                .reason;
            assert_eq!(
                msg,
                "No opposing orders for 15871154585880608648532107628464183779895785213830018178010423617714102767076 which means there is no market price"
            );
            Ok(())
        }

        #[tokio::test]
        async fn market_buy_with_shares_fok_should_fail_on_insufficient_liquidity()
        -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            // only 50 shares available on asks
            ensure_requirements_for_market_price(
                &server,
                token_1(),
                &[],
                &[OrderSummary::builder()
                    .price(dec!(0.5))
                    .size(dec!(50))
                    .build()],
            );

            let err = client
                .market_order()
                .token_id(token_1())
                .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
                .side(Side::Buy)
                .order_type(OrderType::FOK)
                .build()
                .await
                .unwrap_err();

            let msg = &err
                .downcast_ref::<polymarket_client_sdk_v2::error::Validation>()
                .unwrap()
                .reason;
            assert_eq!(
                msg,
                "Insufficient liquidity to fill order for 15871154585880608648532107628464183779895785213830018178010423617714102767076 at 100"
            );
            Ok(())
        }

        #[tokio::test]
        async fn market_buy_with_shares_should_succeed_and_encode_maker_as_usdc()
        -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            // cutoff price should end at 0.4 for 250 shares
            ensure_requirements_for_market_price(
                &server,
                token_1(),
                &[],
                &[
                    OrderSummary::builder()
                        .price(dec!(0.5))
                        .size(dec!(100))
                        .build(),
                    OrderSummary::builder()
                        .price(dec!(0.4))
                        .size(dec!(300))
                        .build(),
                ],
            );

            let signable_order = client
                .market_order()
                .token_id(token_1())
                .amount(Amount::shares(dec!(250))?)
                .side(Side::Buy)
                .order_type(OrderType::FOK)
                .build()
                .await?;

            // maker = USDC, taker = shares
            assert_eq!(signable_order.order().makerAmount, U256::from(100_000_000)); // 250 * 0.4 = 100
            assert_eq!(signable_order.order().takerAmount, U256::from(250_000_000));
            Ok(())
        }

        #[tokio::test]
        async fn market_buy_with_price_should_succeed() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            // cutoff price should end at 0.4 for 250 shares
            ensure_requirements_for_market_price(
                &server,
                token_1(),
                &[],
                &[
                    OrderSummary::builder()
                        .price(dec!(0.5))
                        .size(dec!(100))
                        .build(),
                    OrderSummary::builder()
                        .price(dec!(0.4))
                        .size(dec!(300))
                        .build(),
                ],
            );

            let signable_order = client
                .market_order()
                .token_id(token_1())
                .amount(Amount::shares(dec!(250))?)
                .side(Side::Buy)
                .price(dec!(0.5))
                .order_type(OrderType::FOK)
                .build()
                .await?;

            // maker = USDC, taker = shares
            assert_eq!(signable_order.order().makerAmount, U256::from(125_000_000)); // 250 * 0.5 = 125
            assert_eq!(signable_order.order().takerAmount, U256::from(250_000_000));
            Ok(())
        }
    }

    mod sell {
        use super::*;

        mod fok {
            use super::*;

            #[tokio::test]
            async fn should_fail_on_no_bids() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(&server, token_1(), &[], &[]);

                let err = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
                    .side(Side::Sell)
                    .order_type(OrderType::FOK)
                    .build()
                    .await
                    .unwrap_err();
                let msg = &err.downcast_ref::<Validation>().unwrap().reason;

                assert_eq!(
                    msg,
                    "No opposing orders for 15871154585880608648532107628464183779895785213830018178010423617714102767076 which means there is no market price"
                );

                Ok(())
            }

            #[tokio::test]
            async fn should_fail_on_insufficient_liquidity() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(Decimal::TEN)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::TEN)
                            .build(),
                    ],
                    &[],
                );

                let err = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
                    .side(Side::Sell)
                    .order_type(OrderType::FOK)
                    .build()
                    .await
                    .unwrap_err();
                let msg = &err.downcast_ref::<Validation>().unwrap().reason;

                assert_eq!(
                    msg,
                    "Insufficient liquidity to fill order for 15871154585880608648532107628464183779895785213830018178010423617714102767076 at 100"
                );

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                    ],
                    &[],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
                    .side(Side::Sell)
                    .order_type(OrderType::FOK)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(taker_amount) / to_decimal(maker_amount);
                assert_eq!(price, dec!(0.5));

                assert_eq!(signable_order.order().maker, client.address());
                assert_eq!(signable_order.order().signer, client.address());

                assert_eq!(
                    signable_order.order().tokenId,
                    U256::from_str(
                        "15871154585880608648532107628464183779895785213830018178010423617714102767076"
                    )?
                );
                assert_eq!(maker_amount, U256::from(100_000_000)); // 100 `token_1()` tokens
                assert_eq!(taker_amount, U256::from(50_000_000)); // 50 USDC
                assert_eq!(signable_order.payload.expiration, U256::ZERO);

                assert_eq!(signable_order.order().side, Side::Sell as u8);
                assert_eq!(
                    signable_order.order().signatureType,
                    SignatureType::Eoa as u8
                );

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed_2() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(dec!(300))
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::TEN)
                            .build(),
                    ],
                    &[],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
                    .side(Side::Sell)
                    .order_type(OrderType::FOK)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(taker_amount) / to_decimal(maker_amount);
                assert_eq!(price, dec!(0.4));

                assert_eq!(maker_amount, U256::from(100_000_000)); // 100 `token_1()` tokens
                assert_eq!(taker_amount, U256::from(40_000_000)); // 40 USDC

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed_3() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(dec!(200))
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::TEN)
                            .build(),
                    ],
                    &[],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::shares(dec!(200))?)
                    .side(Side::Sell)
                    .order_type(OrderType::FOK)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(taker_amount) / to_decimal(maker_amount);
                assert_eq!(price, dec!(0.4));

                assert_eq!(maker_amount, U256::from(200_000_000)); // 200 `token_1()` tokens
                assert_eq!(taker_amount, U256::from(80_000_000)); // 80 USDC

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed_4() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(dec!(300))
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                    ],
                    &[],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::shares(dec!(300))?)
                    .side(Side::Sell)
                    .order_type(OrderType::FOK)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(taker_amount) / to_decimal(maker_amount);
                assert_eq!(price, dec!(0.3));

                assert_eq!(maker_amount, U256::from(300_000_000)); // 300 `token_1()` tokens
                assert_eq!(taker_amount, U256::from(90_000_000)); // 90 USDC

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed_5() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(dec!(334))
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                    ],
                    &[],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::shares(dec!(300))?)
                    .side(Side::Sell)
                    .order_type(OrderType::FOK)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(taker_amount) / to_decimal(maker_amount);
                assert_eq!(price, dec!(0.3));

                assert_eq!(maker_amount, U256::from(300_000_000)); // 300 `token_1()` tokens
                assert_eq!(taker_amount, U256::from(90_000_000)); // 90 USDC

                Ok(())
            }
        }

        mod fak {
            use super::*;

            #[tokio::test]
            async fn should_fail_on_no_bids() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(&server, token_1(), &[], &[]);

                let err = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
                    .side(Side::Sell)
                    .build()
                    .await
                    .unwrap_err();
                let msg = &err.downcast_ref::<Validation>().unwrap().reason;

                assert_eq!(
                    msg,
                    "No opposing orders for 15871154585880608648532107628464183779895785213830018178010423617714102767076 which means there is no market price"
                );

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(Decimal::TEN)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::TEN)
                            .build(),
                    ],
                    &[],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
                    .side(Side::Sell)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(taker_amount) / to_decimal(maker_amount);
                assert_eq!(price, dec!(0.4));

                assert_eq!(signable_order.order().maker, client.address());
                assert_eq!(signable_order.order().signer, client.address());

                assert_eq!(
                    signable_order.order().tokenId,
                    U256::from_str(
                        "15871154585880608648532107628464183779895785213830018178010423617714102767076"
                    )?
                );
                assert_eq!(signable_order.order().makerAmount, U256::from(100_000_000)); // 100 USDC
                assert_eq!(signable_order.order().takerAmount, U256::from(40_000_000)); // 40 `token_1()` tokens
                assert_eq!(signable_order.payload.expiration, U256::ZERO);

                assert_eq!(signable_order.order().side, Side::Sell as u8);
                assert_eq!(
                    signable_order.order().signatureType,
                    SignatureType::Eoa as u8
                );

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed_2() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                    ],
                    &[],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
                    .side(Side::Sell)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(taker_amount) / to_decimal(maker_amount);
                assert_eq!(price, dec!(0.5));

                assert_eq!(maker_amount, U256::from(100_000_000)); // 100 `token_1()` tokens
                assert_eq!(taker_amount, U256::from(50_000_000)); // 50 USDC

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed_3() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(dec!(300))
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::TEN)
                            .build(),
                    ],
                    &[],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
                    .side(Side::Sell)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(taker_amount) / to_decimal(maker_amount);
                assert_eq!(price, dec!(0.4));

                assert_eq!(maker_amount, U256::from(100_000_000)); // 100 `token_1()` tokens
                assert_eq!(taker_amount, U256::from(40_000_000)); // 40 USDC

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed_4() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(dec!(200))
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::TEN)
                            .build(),
                    ],
                    &[],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::shares(dec!(200))?)
                    .side(Side::Sell)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(taker_amount) / to_decimal(maker_amount);
                assert_eq!(price, dec!(0.4));

                assert_eq!(maker_amount, U256::from(200_000_000)); // 200 `token_1()` tokens
                assert_eq!(taker_amount, U256::from(80_000_000)); // 80 USDC

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed_5() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(dec!(300))
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                    ],
                    &[],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
                    .side(Side::Sell)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(taker_amount) / to_decimal(maker_amount);
                assert_eq!(price, dec!(0.5));

                assert_eq!(maker_amount, U256::from(100_000_000)); // 100 `token_1()` tokens
                assert_eq!(taker_amount, U256::from(50_000_000)); // 50 USDC

                Ok(())
            }

            #[tokio::test]
            async fn should_succeed_6() -> anyhow::Result<()> {
                let server = MockServer::start();
                let client = create_authenticated(&server).await?;

                ensure_requirements_for_market_price(
                    &server,
                    token_1(),
                    &[
                        OrderSummary::builder()
                            .price(dec!(0.3))
                            .size(dec!(334))
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.4))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                        OrderSummary::builder()
                            .price(dec!(0.5))
                            .size(Decimal::ONE_HUNDRED)
                            .build(),
                    ],
                    &[],
                );

                let signable_order = client
                    .market_order()
                    .token_id(token_1())
                    .amount(Amount::shares(dec!(300))?)
                    .side(Side::Sell)
                    .build()
                    .await?;

                let maker_amount = signable_order.order().makerAmount;
                let taker_amount = signable_order.order().takerAmount;

                let price = to_decimal(taker_amount) / to_decimal(maker_amount);
                assert_eq!(price, dec!(0.3));

                assert_eq!(maker_amount, U256::from(300_000_000)); // 300 `token_1()` tokens
                assert_eq!(taker_amount, U256::from(90_000_000)); // 90 USDC

                Ok(())
            }
        }

        #[tokio::test]
        async fn should_succeed_0_1() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Tenth);
            // Always gives a market price of 0.5 for 100
            ensure_requirements_for_market_price(
                &server,
                token_1(),
                &[OrderSummary::builder()
                    .price(dec!(0.5))
                    .size(Decimal::ONE_HUNDRED)
                    .build()],
                &[],
            );

            let signable_order = client
                .market_order()
                .token_id(token_1())
                .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
                .side(Side::Sell)
                .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
                .build()
                .await?;

            let maker_amount = signable_order.order().makerAmount;
            let taker_amount = signable_order.order().takerAmount;

            let price = to_decimal(taker_amount) / to_decimal(maker_amount);
            assert_eq!(price, dec!(0.50));

            assert_eq!(signable_order.order().maker, client.address());
            assert_eq!(signable_order.order().signer, client.address());

            assert_eq!(signable_order.order().tokenId, token_1());
            assert_eq!(signable_order.order().makerAmount, U256::from(100_000_000));
            assert_eq!(signable_order.order().takerAmount, U256::from(50_000_000));
            assert_eq!(signable_order.payload.expiration, U256::from(0));

            assert_eq!(signable_order.order().side, Side::Sell as u8);
            assert_eq!(
                signable_order.order().signatureType,
                SignatureType::Eoa as u8
            );

            Ok(())
        }

        #[tokio::test]
        async fn should_succeed_0_01() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Hundredth);
            // Always gives a market price of 0.56 for 100
            ensure_requirements_for_market_price(
                &server,
                token_1(),
                &[OrderSummary::builder()
                    .price(dec!(0.56))
                    .size(Decimal::ONE_HUNDRED)
                    .build()],
                &[],
            );

            let signable_order = client
                .market_order()
                .token_id(token_1())
                .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
                .side(Side::Sell)
                .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
                .build()
                .await?;

            let maker_amount = signable_order.order().makerAmount;
            let taker_amount = signable_order.order().takerAmount;

            let price = (to_decimal(taker_amount) / to_decimal(maker_amount))
                .trunc_with_scale(USDC_DECIMALS);
            assert_eq!(price, dec!(0.56));

            assert_eq!(signable_order.order().maker, client.address());
            assert_eq!(signable_order.order().signer, client.address());

            assert_eq!(signable_order.order().tokenId, token_1());
            assert_eq!(signable_order.order().makerAmount, U256::from(100_000_000));
            assert_eq!(signable_order.order().takerAmount, U256::from(56_000_000));
            assert_eq!(signable_order.payload.expiration, U256::from(0));

            assert_eq!(signable_order.order().side, Side::Sell as u8);
            assert_eq!(
                signable_order.order().signatureType,
                SignatureType::Eoa as u8
            );

            Ok(())
        }

        #[tokio::test]
        async fn should_succeed_0_001() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::Thousandth);
            // Always gives a market price of 0.056 for 100
            ensure_requirements_for_market_price(
                &server,
                token_1(),
                &[OrderSummary::builder()
                    .price(dec!(0.056))
                    .size(Decimal::ONE_HUNDRED)
                    .build()],
                &[],
            );

            let signable_order = client
                .market_order()
                .token_id(token_1())
                .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
                .side(Side::Sell)
                .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
                .build()
                .await?;

            let maker_amount = signable_order.order().makerAmount;
            let taker_amount = signable_order.order().takerAmount;

            let price = (to_decimal(taker_amount) / to_decimal(maker_amount))
                .trunc_with_scale(USDC_DECIMALS);
            assert_eq!(price, dec!(0.056));

            assert_eq!(signable_order.order().maker, client.address());
            assert_eq!(signable_order.order().signer, client.address());

            assert_eq!(signable_order.order().tokenId, token_1());
            assert_eq!(signable_order.order().makerAmount, U256::from(100_000_000));
            assert_eq!(signable_order.order().takerAmount, U256::from(5_600_000));
            assert_eq!(signable_order.payload.expiration, U256::from(0));

            assert_eq!(signable_order.order().side, Side::Sell as u8);
            assert_eq!(
                signable_order.order().signatureType,
                SignatureType::Eoa as u8
            );

            Ok(())
        }

        #[tokio::test]
        async fn should_succeed_0_0001() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements(&server, token_1(), TickSize::TenThousandth);
            // Always gives a market price of 0.0056 for 100
            ensure_requirements_for_market_price(
                &server,
                token_1(),
                &[OrderSummary::builder()
                    .price(dec!(0.0056))
                    .size(Decimal::ONE_HUNDRED)
                    .build()],
                &[],
            );

            let signable_order = client
                .market_order()
                .token_id(token_1())
                .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
                .side(Side::Sell)
                .expiration(DateTime::<Utc>::from_str("1970-01-01T13:53:20Z").unwrap())
                .build()
                .await?;

            let maker_amount = signable_order.order().makerAmount;
            let taker_amount = signable_order.order().takerAmount;

            let price = (to_decimal(taker_amount) / to_decimal(maker_amount))
                .trunc_with_scale(USDC_DECIMALS);
            assert_eq!(price, dec!(0.0056));

            assert_eq!(signable_order.order().maker, client.address());
            assert_eq!(signable_order.order().signer, client.address());

            assert_eq!(signable_order.order().tokenId, token_1());
            assert_eq!(signable_order.order().makerAmount, U256::from(100_000_000));
            assert_eq!(signable_order.order().takerAmount, U256::from(560_000));
            assert_eq!(signable_order.payload.expiration, U256::from(0));

            assert_eq!(signable_order.order().side, Side::Sell as u8);
            assert_eq!(
                signable_order.order().signatureType,
                SignatureType::Eoa as u8
            );

            Ok(())
        }
    }

    #[tokio::test]
    async fn should_fail_on_missing_required_fields() -> anyhow::Result<()> {
        let server = MockServer::start();
        let client = create_authenticated(&server).await?;

        let err = client
            .market_order()
            .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
            .side(Side::Buy)
            .build()
            .await
            .unwrap_err();
        let msg = &err.downcast_ref::<Validation>().unwrap().reason;

        assert_eq!(msg, "Unable to build Order due to missing token ID");

        let err = client
            .market_order()
            .token_id(token_1())
            .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
            .build()
            .await
            .unwrap_err();
        let msg = &err.downcast_ref::<Validation>().unwrap().reason;

        assert_eq!(msg, "Unable to build Order due to missing token side");

        let err = client
            .market_order()
            .token_id(token_1())
            .side(Side::Buy)
            .build()
            .await
            .unwrap_err();
        let msg = &err.downcast_ref::<Validation>().unwrap().reason;

        assert_eq!(msg, "Unable to build Order due to missing amount");

        Ok(())
    }

    #[tokio::test]
    async fn should_fail_on_gtc() -> anyhow::Result<()> {
        let server = MockServer::start();
        let client = create_authenticated(&server).await?;

        ensure_requirements_for_market_price(&server, token_1(), &[], &[]);

        let err = client
            .market_order()
            .token_id(token_1())
            .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
            .side(Side::Sell)
            .order_type(OrderType::GTC)
            .build()
            .await
            .unwrap_err();
        let msg = &err.downcast_ref::<Validation>().unwrap().reason;

        assert_eq!(
            msg,
            "Cannot set an order type other than FAK/FOK for a market order"
        );

        Ok(())
    }

    #[tokio::test]
    async fn market_sell_with_usdc_should_fail() -> anyhow::Result<()> {
        let server = MockServer::start();
        let client = create_authenticated(&server).await?;

        ensure_requirements_for_market_price(&server, token_1(), &[], &[]);

        let err = client
            .market_order()
            .token_id(token_1())
            .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
            .side(Side::Sell)
            .build()
            .await
            .unwrap_err();
        let msg = &err
            .downcast_ref::<polymarket_client_sdk_v2::error::Validation>()
            .unwrap()
            .reason;

        assert_eq!(msg, "Sell Orders must specify their `amount`s in shares");
        Ok(())
    }
}

/// V2 order tests — covers the new V2 order structure, fields, serialization,
/// validation, and builder behavior.
mod v2 {
    use std::str::FromStr as _;

    use alloy::primitives::U256;
    use alloy::signers::Signer as _;
    use alloy::signers::local::LocalSigner;
    use polymarket_client_sdk_v2::POLYGON;
    use polymarket_client_sdk_v2::clob::types::response::OrderSummary;
    use polymarket_client_sdk_v2::clob::{Client, Config};
    use polymarket_client_sdk_v2::error::Validation;
    use serde_json::json;

    use super::*;
    use crate::common::{
        API_KEY, PASSPHRASE, POLY_ADDRESS, PRIVATE_KEY, SECRET, create_authenticated, token_1,
        token_2,
    };

    /// V2 orders only need neg-risk + tick-size mocks (no fee-rate).
    fn ensure_requirements_v2(server: &MockServer, token_id: U256, tick_size: TickSize) {
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/neg-risk");
            then.status(StatusCode::OK)
                .json_body(json!({ "neg_risk": false }));
        });

        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/tick-size")
                .query_param("token_id", token_id.to_string());
            then.status(StatusCode::OK).json_body(json!({
                "minimum_tick_size": tick_size.as_decimal(),
            }));
        });
    }

    mod limit {
        use super::*;

        #[tokio::test]
        async fn v2_limit_buy_should_succeed() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements_v2(&server, token_1(), TickSize::Hundredth);

            let signable = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.50))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .build()
                .await?;

            // Should be a V2 payload (default)
            let order = &signable.payload.order;
            let expiration = &signable.payload.expiration;
            assert_eq!(order.side, Side::Buy as u8);
            assert_eq!(order.signatureType, SignatureType::Eoa as u8);
            assert_eq!(order.metadata, B256::ZERO);
            assert_eq!(order.builder, B256::ZERO);
            assert!(!order.timestamp.is_zero(), "timestamp should be set");
            assert_eq!(*expiration, U256::ZERO, "default expiration is zero");

            // maker_amount = 100 * 0.50 = 50 USDC = 50_000_000
            assert_eq!(order.makerAmount, U256::from(50_000_000u64));
            // taker_amount = 100 shares = 100_000_000
            assert_eq!(order.takerAmount, U256::from(100_000_000u64));

            Ok(())
        }

        #[tokio::test]
        async fn v2_limit_sell_should_succeed() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements_v2(&server, token_1(), TickSize::Hundredth);

            let signable = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.34))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Sell)
                .build()
                .await?;

            let order = &signable.payload.order;
            assert_eq!(order.side, Side::Sell as u8);
            // maker_amount = 100 shares = 100_000_000
            assert_eq!(order.makerAmount, U256::from(100_000_000u64));
            // taker_amount = 100 * 0.34 = 34 USDC = 34_000_000
            assert_eq!(order.takerAmount, U256::from(34_000_000u64));

            Ok(())
        }

        #[tokio::test]
        async fn v2_with_metadata_and_builder_code() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements_v2(&server, token_1(), TickSize::Hundredth);

            let metadata = B256::from([0xAB; 32]);
            let builder_code = B256::from([0xCD; 32]);

            let signable = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.50))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .metadata(metadata)
                .builder_code(builder_code)
                .build()
                .await?;

            let order = &signable.payload.order;
            assert_eq!(order.metadata, metadata);
            assert_eq!(order.builder, builder_code);

            Ok(())
        }

        #[tokio::test]
        async fn v2_with_expiration_gtd() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements_v2(&server, token_1(), TickSize::Hundredth);

            let expiration = DateTime::from_timestamp(1_700_000_000, 0).expect("valid timestamp");

            let signable = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.50))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .order_type(OrderType::GTD)
                .expiration(expiration)
                .build()
                .await?;

            let exp = &signable.payload.expiration;
            assert_eq!(*exp, U256::from(1_700_000_000u64));
            assert_eq!(signable.order_type, OrderType::GTD);

            Ok(())
        }

        #[tokio::test]
        async fn v2_with_defer_exec() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements_v2(&server, token_1(), TickSize::Hundredth);

            let signable = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.50))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .defer_exec(true)
                .build()
                .await?;

            assert_eq!(signable.defer_exec, Some(true));

            Ok(())
        }

        #[tokio::test]
        async fn v2_with_funder_and_proxy_signature() -> anyhow::Result<()> {
            let server = MockServer::start();

            let signer = LocalSigner::from_str(PRIVATE_KEY)?.with_chain_id(Some(POLYGON));
            server.mock(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/auth/derive-api-key")
                    .header(POLY_ADDRESS, signer.address().to_string().to_lowercase());
                then.status(StatusCode::OK).json_body(json!({
                    "apiKey": API_KEY.to_string(),
                    "passphrase": PASSPHRASE,
                    "secret": SECRET
                }));
            });

            let funder = address!("0xaDEFf2158d668f64308C62ef227C5CcaCAAf976D");
            let client = Client::new(&server.base_url(), Config::default())?
                .authentication_builder(&signer)
                .funder(funder)
                .signature_type(SignatureType::Proxy)
                .authenticate()
                .await?;

            ensure_requirements_v2(&server, token_1(), TickSize::Hundredth);

            let signable = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.50))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .build()
                .await?;

            let order = &signable.payload.order;
            assert_eq!(order.maker, funder);
            assert_eq!(order.signer, signer.address());
            assert_ne!(order.maker, order.signer);
            assert_eq!(order.signatureType, SignatureType::Proxy as u8);

            Ok(())
        }

        #[tokio::test]
        async fn v2_with_poly1271_signature() -> anyhow::Result<()> {
            let server = MockServer::start();

            let signer = LocalSigner::from_str(PRIVATE_KEY)?.with_chain_id(Some(POLYGON));
            server.mock(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/auth/derive-api-key")
                    .header(POLY_ADDRESS, signer.address().to_string().to_lowercase());
                then.status(StatusCode::OK).json_body(json!({
                    "apiKey": API_KEY.to_string(),
                    "passphrase": PASSPHRASE,
                    "secret": SECRET
                }));
            });

            let funder = address!("0xaDEFf2158d668f64308C62ef227C5CcaCAAf976D");
            let client = Client::new(&server.base_url(), Config::default())?
                .authentication_builder(&signer)
                .funder(funder)
                .signature_type(SignatureType::Poly1271)
                .authenticate()
                .await?;

            ensure_requirements_v2(&server, token_1(), TickSize::Hundredth);

            let signable = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.50))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .build()
                .await?;

            let order = &signable.payload.order;
            assert_eq!(order.signatureType, SignatureType::Poly1271 as u8);

            Ok(())
        }

        #[tokio::test]
        async fn v2_price_validation_same_as_v1() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements_v2(&server, token_1(), TickSize::Hundredth);

            // Too many decimal places for tick size
            let err = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.001))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .build()
                .await
                .unwrap_err();
            let msg = &err.downcast_ref::<Validation>().unwrap().reason;
            assert!(msg.contains("decimal places"));

            // Price at boundary (1 - tick_size = 0.99) should succeed
            // Price beyond boundary should fail
            ensure_requirements_v2(&server, token_2(), TickSize::Tenth);

            let err = client
                .limit_order()
                .token_id(token_2())
                .price(dec!(0.9))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .build()
                .await;
            // 0.9 == 1 - 0.1, so it's at the boundary — should succeed
            assert!(err.is_ok());

            // Price below minimum tick size
            let err = client
                .limit_order()
                .token_id(token_1())
                .price(Decimal::ZERO)
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .build()
                .await
                .unwrap_err();
            let msg = &err.downcast_ref::<Validation>().unwrap().reason;
            assert!(msg.contains("too small or too large"));

            // Negative price
            let err = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(-0.50))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .build()
                .await
                .unwrap_err();
            let msg = &err.downcast_ref::<Validation>().unwrap().reason;
            assert!(msg.contains("negative price"));

            Ok(())
        }

        #[tokio::test]
        async fn v2_salt_generator_propagates() -> anyhow::Result<()> {
            let server = MockServer::start();

            let signer = LocalSigner::from_str(PRIVATE_KEY)?.with_chain_id(Some(POLYGON));
            server.mock(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/auth/derive-api-key");
                then.status(StatusCode::OK).json_body(json!({
                    "apiKey": API_KEY.to_string(),
                    "passphrase": PASSPHRASE,
                    "secret": SECRET
                }));
            });

            let client = Client::new(&server.base_url(), Config::default())?
                .authentication_builder(&signer)
                .salt_generator(|| 42)
                .authenticate()
                .await?;

            ensure_requirements_v2(&server, token_1(), TickSize::Hundredth);

            let signable = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.50))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .build()
                .await?;

            let order = &signable.payload.order;
            assert_eq!(order.salt, U256::from(42));

            Ok(())
        }

        #[tokio::test]
        async fn v2_different_tick_sizes() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            // Tenth tick size
            ensure_requirements_v2(&server, token_1(), TickSize::Tenth);

            let signable = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.5))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .build()
                .await?;

            let order = &signable.payload.order;
            // 100 * 0.5 = 50 USDC = 50_000_000
            assert_eq!(order.makerAmount, U256::from(50_000_000u64));

            // Thousandth tick size
            ensure_requirements_v2(&server, token_2(), TickSize::Thousandth);

            let signable = client
                .limit_order()
                .token_id(token_2())
                .price(dec!(0.512))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .build()
                .await?;

            let order = &signable.payload.order;
            // 100 * 0.512 = 51.2 USDC = 51_200_000
            assert_eq!(order.makerAmount, U256::from(51_200_000u64));

            Ok(())
        }

        #[tokio::test]
        async fn v2_post_only_validation() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            ensure_requirements_v2(&server, token_1(), TickSize::Hundredth);

            // postOnly with FOK should fail
            let err = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.50))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .order_type(OrderType::FOK)
                .post_only(true)
                .build()
                .await
                .unwrap_err();

            let msg = &err.downcast_ref::<Validation>().unwrap().reason;
            assert_eq!(msg, "postOnly is only supported for GTC and GTD orders");

            // postOnly with GTC should succeed
            let signable = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.50))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .order_type(OrderType::GTC)
                .post_only(true)
                .build()
                .await?;

            assert_eq!(signable.post_only, Some(true));

            Ok(())
        }
    }

    mod market {
        use super::*;

        fn ensure_requirements_for_market_price_v2(
            server: &MockServer,
            token_id: U256,
            bids: &[OrderSummary],
            asks: &[OrderSummary],
        ) {
            let minimum_tick_size = TickSize::Tenth;

            server.mock(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/book")
                    .query_param("token_id", token_id.to_string());
                then.status(StatusCode::OK).json_body(json!({
                    "market": "0xbd31dc8a20211944f6b70f31557f1001557b59905b7738480ca09bd4532f84af",
                    "asset_id": token_id,
                    "timestamp": "1000",
                    "bids": bids,
                    "asks": asks,
                    "min_order_size": "5",
                    "neg_risk": false,
                    "tick_size": minimum_tick_size.as_decimal(),
                }));
            });

            server.mock(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/tick-size")
                    .query_param("token_id", token_id.to_string());
                then.status(StatusCode::OK).json_body(json!({
                    "minimum_tick_size": minimum_tick_size.as_decimal(),
                }));
            });
        }

        #[tokio::test]
        async fn v2_market_buy_usdc() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            let asks = vec![
                OrderSummary::builder()
                    .price(dec!(0.4))
                    .size(dec!(200.0))
                    .build(),
                OrderSummary::builder()
                    .price(dec!(0.5))
                    .size(dec!(200.0))
                    .build(),
            ];

            ensure_requirements_for_market_price_v2(&server, token_1(), &[], &asks);

            let signable = client
                .market_order()
                .token_id(token_1())
                .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                .side(Side::Buy)
                .build()
                .await?;

            let order = &signable.payload.order;
            let expiration = &signable.payload.expiration;
            assert_eq!(order.side, Side::Buy as u8);
            assert_eq!(*expiration, U256::ZERO);
            assert!(!order.timestamp.is_zero());
            assert_eq!(order.metadata, B256::ZERO);
            assert_eq!(order.builder, B256::ZERO);
            // amount = 100 USDC, price = 0.5 (cutoff), shares = 100/0.5 = 200
            assert_eq!(order.makerAmount, U256::from(100_000_000u64));
            assert_eq!(order.takerAmount, U256::from(200_000_000u64));

            Ok(())
        }

        #[tokio::test]
        async fn v2_market_sell_shares() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            let bids = vec![
                OrderSummary::builder()
                    .price(dec!(0.3))
                    .size(dec!(200.0))
                    .build(),
                OrderSummary::builder()
                    .price(dec!(0.4))
                    .size(dec!(200.0))
                    .build(),
            ];

            ensure_requirements_for_market_price_v2(&server, token_1(), &bids, &[]);

            let signable = client
                .market_order()
                .token_id(token_1())
                .amount(Amount::shares(Decimal::ONE_HUNDRED)?)
                .side(Side::Sell)
                .build()
                .await?;

            let order = &signable.payload.order;
            assert_eq!(order.side, Side::Sell as u8);
            // maker = 100 shares, taker = 100 * 0.4 (cutoff) = 40 USDC
            assert_eq!(order.makerAmount, U256::from(100_000_000u64));
            assert_eq!(order.takerAmount, U256::from(40_000_000u64));

            Ok(())
        }

        #[tokio::test]
        async fn v2_market_with_metadata_and_builder() -> anyhow::Result<()> {
            let server = MockServer::start();
            let client = create_authenticated(&server).await?;

            let asks = vec![
                OrderSummary::builder()
                    .price(dec!(0.5))
                    .size(dec!(500.0))
                    .build(),
            ];

            ensure_requirements_for_market_price_v2(&server, token_1(), &[], &asks);

            let metadata = B256::from([0x11; 32]);
            let builder_code = B256::from([0x22; 32]);

            let signable = client
                .market_order()
                .token_id(token_1())
                .amount(Amount::usdc(Decimal::ONE_HUNDRED)?)
                .side(Side::Buy)
                .metadata(metadata)
                .builder_code(builder_code)
                .defer_exec(true)
                .build()
                .await?;

            let order = &signable.payload.order;
            assert_eq!(order.metadata, metadata);
            assert_eq!(order.builder, builder_code);
            assert_eq!(signable.defer_exec, Some(true));

            Ok(())
        }
    }

    mod serialization {
        use alloy::primitives::Signature;
        use polymarket_client_sdk_v2::clob::types::{OrderPayload, SignedOrder};
        use serde_json::to_value;

        use super::*;

        #[test]
        fn signed_order_json_structure() {
            let mut order = polymarket_client_sdk_v2::clob::types::Order::default();
            order.salt = U256::from(12345u64);
            order.maker = address!("0xaDEFf2158d668f64308C62ef227C5CcaCAAf976D");
            order.signer = address!("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266");
            order.tokenId = U256::from(999u64);
            order.makerAmount = U256::from(50_000_000u64);
            order.takerAmount = U256::from(100_000_000u64);
            order.side = Side::Buy as u8;
            order.signatureType = SignatureType::Eoa as u8;
            order.timestamp = U256::from(1700000000000u64);
            order.metadata = B256::from([0xAA; 32]);
            order.builder = B256::from([0xBB; 32]);

            let signed = SignedOrder::builder()
                .payload(OrderPayload::new(order, U256::from(1700001000u64)))
                .signature(Signature::new(U256::from(1u64), U256::from(2u64), false))
                .order_type(OrderType::GTC)
                .owner(API_KEY)
                .post_only(false)
                .defer_exec(true)
                .build();

            let value = to_value(&signed).unwrap();
            let obj = value.as_object().unwrap();

            // Top-level fields
            assert_eq!(obj["orderType"], "GTC");
            assert_eq!(obj["deferExec"], true);
            assert_eq!(obj["postOnly"], false);

            // Order object
            let order_obj = obj["order"].as_object().unwrap();
            assert_eq!(order_obj["salt"], 12345);
            assert_eq!(
                order_obj["maker"],
                "0xadeff2158d668f64308c62ef227c5ccacaaf976d"
            );
            assert_eq!(
                order_obj["signer"],
                "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
            );
            assert_eq!(order_obj["tokenId"], "999");
            assert_eq!(order_obj["makerAmount"], "50000000");
            assert_eq!(order_obj["takerAmount"], "100000000");
            assert_eq!(order_obj["side"], "BUY");
            assert_eq!(order_obj["signatureType"], 0);
            assert_eq!(order_obj["timestamp"], "1700000000000");
            assert_eq!(order_obj["expiration"], "1700001000");

            // metadata and builder should be hex-encoded bytes32
            assert!(order_obj.contains_key("metadata"));
            assert!(order_obj.contains_key("builder"));

            // Must NOT contain removed V1-only fields
            assert!(!order_obj.contains_key("taker"));
            assert!(!order_obj.contains_key("nonce"));
            assert!(!order_obj.contains_key("feeRateBps"));

            // Must have signature
            assert!(order_obj.contains_key("signature"));
        }

        #[test]
        fn signed_order_omits_optional_fields_when_none() {
            let signed = SignedOrder::builder()
                .payload(OrderPayload::new(
                    polymarket_client_sdk_v2::clob::types::Order::default(),
                    U256::ZERO,
                ))
                .signature(Signature::new(U256::ZERO, U256::ZERO, false))
                .order_type(OrderType::FOK)
                .owner(API_KEY)
                .build();

            let value = to_value(&signed).unwrap();
            let obj = value.as_object().unwrap();

            assert!(!obj.contains_key("postOnly"));
            assert!(!obj.contains_key("deferExec"));
        }
    }

    mod signing {
        use alloy::signers::Signer as _;
        use alloy::signers::local::LocalSigner;
        use polymarket_client_sdk_v2::POLYGON;
        use polymarket_client_sdk_v2::clob::{Client, Config};
        use serde_json::json;

        use super::*;
        use crate::common::{API_KEY, PASSPHRASE, POLY_ADDRESS, PRIVATE_KEY, SECRET};

        #[tokio::test]
        async fn v2_sign_produces_valid_signature() -> anyhow::Result<()> {
            let server = MockServer::start();

            let signer = LocalSigner::from_str(PRIVATE_KEY)?.with_chain_id(Some(POLYGON));
            server.mock(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/auth/derive-api-key")
                    .header(POLY_ADDRESS, signer.address().to_string().to_lowercase());
                then.status(StatusCode::OK).json_body(json!({
                    "apiKey": API_KEY.to_string(),
                    "passphrase": PASSPHRASE,
                    "secret": SECRET
                }));
            });

            let client = Client::new(&server.base_url(), Config::default())?
                .authentication_builder(&signer)
                .salt_generator(|| 1)
                .authenticate()
                .await?;

            ensure_requirements_v2(&server, token_1(), TickSize::Hundredth);

            // Need neg-risk mock for sign()
            server.mock(|when, then| {
                when.method(httpmock::Method::GET).path("/neg-risk");
                then.status(StatusCode::OK)
                    .json_body(json!({ "neg_risk": false }));
            });

            let signable = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.50))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .build()
                .await?;

            let signed = client.sign(&signer, signable).await?;

            // Verify the signature is non-zero
            assert_ne!(signed.signature.r(), U256::ZERO);
            assert_ne!(signed.signature.s(), U256::ZERO);

            // Verify owner is set
            assert_eq!(signed.owner, API_KEY);

            Ok(())
        }

        #[tokio::test]
        async fn v2_sign_deterministic_with_fixed_salt() -> anyhow::Result<()> {
            let server = MockServer::start();

            let signer = LocalSigner::from_str(PRIVATE_KEY)?.with_chain_id(Some(POLYGON));
            server.mock(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/auth/derive-api-key");
                then.status(StatusCode::OK).json_body(json!({
                    "apiKey": API_KEY.to_string(),
                    "passphrase": PASSPHRASE,
                    "secret": SECRET
                }));
            });

            let client = Client::new(&server.base_url(), Config::default())?
                .authentication_builder(&signer)
                .salt_generator(|| 1)
                .authenticate()
                .await?;

            ensure_requirements_v2(&server, token_1(), TickSize::Hundredth);

            server.mock(|when, then| {
                when.method(httpmock::Method::GET).path("/neg-risk");
                then.status(StatusCode::OK)
                    .json_body(json!({ "neg_risk": false }));
            });

            // Build two orders with the same params — timestamp will differ
            // but the salt, maker, amounts should be the same
            let signable1 = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.50))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .build()
                .await?;

            let signable2 = client
                .limit_order()
                .token_id(token_1())
                .price(dec!(0.50))
                .size(Decimal::ONE_HUNDRED)
                .side(Side::Buy)
                .build()
                .await?;

            let order1 = &signable1.payload.order;
            let order2 = &signable2.payload.order;

            // Salt should be the same (fixed generator)
            assert_eq!(order1.salt, order2.salt);
            // Amounts should be the same
            assert_eq!(order1.makerAmount, order2.makerAmount);
            assert_eq!(order1.takerAmount, order2.takerAmount);
            // Timestamps may differ (they're set to current time)
            // but both should be non-zero
            assert!(!order1.timestamp.is_zero());
            assert!(!order2.timestamp.is_zero());

            Ok(())
        }
    }
}
