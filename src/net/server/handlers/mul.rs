use crate::helpers::fabric::{ChannelId, MessageChunks, MessageEnvelope};
use crate::helpers::Identity;
use crate::net::server::MpcServerError;
use crate::net::RecordHeaders;
use crate::protocol::{QueryId, RecordId, UniqueStepId};
use async_trait::async_trait;
use axum::extract::{self, FromRequest, Query, RequestParts};
use axum::http::Request;
use hyper::Body;
use tokio_util::sync::PollSender;

/// Used in the axum handler to extract the `query_id` and `step` from the path of the request
pub struct Path(QueryId, UniqueStepId);

#[async_trait]
impl<B: Send> FromRequest<B> for Path {
    type Rejection = MpcServerError;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let extract::Path((query_id, step)) =
            extract::Path::<(QueryId, UniqueStepId)>::from_request(req).await?;
        Ok(Path(query_id, step))
    }
}

#[cfg_attr(feature = "enable-serde", derive(serde::Deserialize))]
pub struct IdentityQuery {
    identity: Identity,
}

/// Injects a permit to send data to the message layer into the Axum request, so that downstream
/// handlers have simple access to the correct value
///
/// For now, stub out the permit logic with just an empty channel
// pub async fn upstream_middleware_fn<B: Send, S: Step>(
//     message_stream: MessageStreamExt<S>,
//     req: Request<B>,
//     next: Next<B>,
// ) -> Result<Response, MpcServerError> {
//     let permit = message_stream.sender.reserve_owned().await?;
//
//     let mut req_parts = RequestParts::new(req);
//     req_parts.extensions_mut().insert(permit);
//
//     let req = req_parts.try_into_request()?;
//
//     Ok(next.run(req).await)
// }

/// accepts all the relevant information from the request, and push all of it onto the gateway
#[allow(clippy::unused_async)] // handler is expected to be async
#[allow(clippy::cast_possible_truncation)] // length of envelopes array known to be less u32
pub async fn handler(
    Path(_query_id, step): Path,
    Query(IdentityQuery { identity }): Query<IdentityQuery>,
    RecordHeaders { offset, data_size }: RecordHeaders,
    mut req: Request<Body>,
) -> Result<(), MpcServerError> {
    // must extract `permit` first since `to_bytes` consumes `req`
    // this also necessitates `take`ing the value out so that we stop borrowing it
    let mut permit = req
        .extensions_mut()
        .get_mut::<Option<PollSender<MessageChunks>>>()
        .unwrap()
        .take()
        .unwrap();

    let channel_id = ChannelId { identity, step };
    let body = hyper::body::to_bytes(req.into_body()).await?;
    let envelopes = body
        .as_ref()
        .chunks(data_size as usize)
        .enumerate()
        .map(|(record_id, chunk)| MessageEnvelope {
            record_id: RecordId::from(offset + record_id as u32),
            payload: chunk.to_vec().into_boxed_slice(),
        })
        .collect::<Vec<_>>();

    permit.send_item((channel_id, envelopes))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{BindTarget, MpcServer};
    use axum::body::Bytes;
    use axum::http::{Request, StatusCode};
    use hyper::{body, Body, Client};
    use tokio::sync::mpsc;

    fn build_req(
        port: u16,
        query_id: QueryId,
        step: UniqueStepId,
        identity: Identity,
        offset: u32,
        data_size: u32,
        body: &'static [u8],
    ) -> Request<Body> {
        assert_eq!(
            body.len() % (data_size as usize),
            0,
            "body len must align with data_size"
        );
        let uri = format!(
            "http://127.0.0.1:{}/mul/query-id/{}/step/{}?identity={}",
            port,
            query_id,
            String::from(step),
            String::from(identity),
        );
        let headers = RecordHeaders {
            offset,
            data_size: data_size as u32,
        };
        let body = Body::from(Bytes::from_static(body));
        headers
            .add_to(Request::post(uri))
            .body(body)
            .expect("request should be valid")
    }

    #[tokio::test]
    async fn collect_req() {
        const DATA_SIZE: u32 = 4;
        const DATA_LEN: usize = 3;

        // initialize server
        let (tx, mut rx) = mpsc::channel(1);
        let server = MpcServer::new(tx);
        let (addr, _) = server
            .bind(BindTarget::Http("127.0.0.1:0".parse().unwrap()))
            .await;
        let port = addr.port();

        // prepare req
        let query_id = QueryId;
        let target_helper = Identity::H2;
        let step = UniqueStepId::default().narrow("test");
        let offset = 0;
        let body = &[0; DATA_LEN * DATA_SIZE as usize];

        let req = build_req(
            port,
            query_id,
            step.clone(),
            target_helper,
            offset,
            DATA_SIZE,
            body,
        );

        // call
        let client = Client::default();
        let resp = client
            .request(req)
            .await
            .expect("client should be able to communicate with server");
        // let service = server.router().into_service();
        // let resp = service.oneshot(req).await.unwrap();

        let status = resp.status();
        let resp_body = body::to_bytes(resp.into_body()).await.unwrap();
        let resp_body_str = String::from_utf8_lossy(&resp_body);

        // response comparison
        let channel_id = ChannelId {
            identity: target_helper,
            step,
        };
        let env = [0; DATA_SIZE as usize].to_vec().into_boxed_slice();
        #[allow(clippy::cast_possible_truncation)] // DATA_LEN is a known size
        let envs = (0..DATA_LEN as u32)
            .map(|i| MessageEnvelope {
                record_id: i.into(),
                payload: env.clone(),
            })
            .collect::<Vec<_>>();

        assert_eq!(status, StatusCode::OK, "{}", resp_body_str);
        let messages = rx.try_recv().expect("should have already received value");
        assert_eq!(messages, (channel_id, envs));
    }
}
