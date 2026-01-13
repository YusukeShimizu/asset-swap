use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use ldk_server_client::client::LdkServerClient;
use ldk_server_protos::api::{Bolt11ReceiveRequest, Bolt11SendRequest, ListPaymentsRequest};
use ldk_server_protos::types::{
    Bolt11InvoiceDescription, PaymentDirection, PaymentStatus, bolt11_invoice_description,
    payment_kind,
};

#[derive(Clone)]
pub struct LdkLightningClient {
    client: LdkServerClient,
}

impl LdkLightningClient {
    pub fn new(rest_service_address: String) -> Self {
        Self {
            client: LdkServerClient::new(rest_service_address),
        }
    }

    pub async fn create_invoice(
        &self,
        amount_msat: u64,
        description: String,
        expiry_secs: u32,
    ) -> Result<String> {
        let description = Bolt11InvoiceDescription {
            kind: Some(bolt11_invoice_description::Kind::Direct(description)),
        };

        let resp = self
            .client
            .bolt11_receive(Bolt11ReceiveRequest {
                amount_msat: Some(amount_msat),
                description: Some(description),
                expiry_secs,
            })
            .await
            .context("Bolt11Receive")?;

        Ok(resp.invoice)
    }

    pub async fn pay_invoice(&self, invoice: String) -> Result<String> {
        let resp = self
            .client
            .bolt11_send(Bolt11SendRequest {
                invoice,
                amount_msat: None,
                route_parameters: None,
            })
            .await
            .context("Bolt11Send")?;
        Ok(resp.payment_id)
    }

    pub async fn wait_preimage(&self, payment_id: &str, timeout: Duration) -> Result<[u8; 32]> {
        let deadline = Instant::now() + timeout;
        loop {
            let payments = self
                .client
                .list_payments(ListPaymentsRequest { page_token: None })
                .await
                .context("ListPayments")?
                .payments;

            if let Some(p) = payments.into_iter().find(|p| p.id == payment_id)
                && p.direction == PaymentDirection::Outbound as i32
                && p.status == PaymentStatus::Succeeded as i32
                && matches!(
                    p.kind.as_ref().and_then(|k| k.kind.as_ref()),
                    Some(payment_kind::Kind::Bolt11(_))
                )
            {
                let preimage_hex = p
                    .kind
                    .and_then(|k| k.kind)
                    .and_then(|k| match k {
                        payment_kind::Kind::Bolt11(b) => b.preimage,
                        _ => None,
                    })
                    .context("missing payment preimage")?;

                let bytes = hex::decode(preimage_hex).context("decode preimage hex")?;
                let preimage: [u8; 32] = bytes
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("preimage must be 32 bytes"))?;
                return Ok(preimage);
            }

            if Instant::now() >= deadline {
                anyhow::bail!("timeout waiting for preimage: payment_id={payment_id}");
            }

            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}
