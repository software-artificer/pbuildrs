mod autogen;

pub use autogen::*;

pub struct CrabService;

#[tonic::async_trait]
impl crabs::crab_service_server::CrabService for CrabService {
    async fn get_ferris(
        &self,
        request: tonic::Request<crabs::GetFerrisReqProto>,
    ) -> Result<tonic::Response<crabs::Ferris>, tonic::Status> {
        match request.into_inner().r#type() {
            crabs::FerrisType::Unknown => Err(tonic::Status::invalid_argument(
                "Unknown Ferris type received",
            )),
            t => Ok(tonic::Response::new(crabs::Ferris { r#type: t.into() })),
        }
    }

    async fn get_mr_krabs(
        &self,
        request: tonic::Request<crabs::GetMrKrabsReqProto>,
    ) -> Result<tonic::Response<crabs::sponge_bob::MrKrabs>, tonic::Status> {
        let state = request.into_inner().state;

        Ok(tonic::Response::new(crabs::sponge_bob::MrKrabs { state }))
    }

    async fn get_sebastian(
        &self,
        request: tonic::Request<crabs::GetSebastianReqProto>,
    ) -> Result<tonic::Response<crabs::disney::ariel::Sebastian>, tonic::Status> {
        let mood = request.into_inner().mood;

        Ok(tonic::Response::new(crabs::disney::ariel::Sebastian {
            mood,
        }))
    }

    async fn get_betsy_krabs(
        &self,
        _: tonic::Request<crabs::GetBetsyKrabsReqProto>,
    ) -> Result<tonic::Response<crabs::sponge_bob::BetsyKrabsProto>, tonic::Status> {
        Err(tonic::Status::unimplemented("This is not yet implemented"))
    }
}

#[cfg(test)]
mod tests {
    use crate::crabs::{self, crab_service_client};

    use super::crabs::crab_service_server;
    use std::net;
    use tokio::net as tokio_net;
    use tokio_stream::wrappers;
    use tonic::transport;

    #[tokio::test]
    async fn test_client_server() {
        let addr: net::SocketAddr = "127.0.0.1:0"
            .parse()
            .expect("Failed to parse socket address");

        let listener = net::TcpListener::bind(addr).expect("Failed to bind TCP listener");
        let port = listener
            .local_addr()
            .expect("Failed to obtain local address form TCP listener")
            .port();

        listener
            .set_nonblocking(true)
            .expect("Failed to change the TCP listener to non-blocking");

        let listener = tokio_net::TcpListener::from_std(listener)
            .expect("Failed to wrap std TCP listener into async Tokio one");
        let stream = wrappers::TcpListenerStream::new(listener);
        let cancel_token = tokio_util::sync::CancellationToken::new();

        let server_cancel_token = cancel_token.clone();
        let server = tokio::spawn(async move {
            transport::Server::builder()
                .add_service(crab_service_server::CrabServiceServer::new(
                    super::CrabService,
                ))
                .serve_with_incoming_shutdown(stream, server_cancel_token.cancelled())
                .await
                .expect("Tonic gRPC server failed");
        });

        let addr = format!("http://127.0.0.1:{port}");
        let mut client = crab_service_client::CrabServiceClient::connect(addr)
            .await
            .expect("Failed to connect to the gRPC server");

        let response = client
            .get_ferris(crabs::GetFerrisReqProto {
                r#type: crabs::FerrisType::Original.into(),
            })
            .await
            .expect("Failed to send a get_ferris request to the gRPC server");
        let ferris_type = response.into_inner().r#type;
        assert_eq!(
            ferris_type,
            crabs::FerrisType::Original.into(),
            "Expected Ferris of Original(1) type, got: {}",
            ferris_type,
        );

        let response = client
            .get_mr_krabs(crabs::GetMrKrabsReqProto {
                state: "Busy".to_string(),
            })
            .await
            .expect("Failed to send a get_mr_krabs request to the gRPC server");
        let state = response.into_inner().state;
        assert_eq!(
            state, "Busy",
            "Invalid MrKrabs state returned by the gRPC server",
        );

        let response = client
            .get_sebastian(crabs::GetSebastianReqProto {
                mood: "Happy".to_string(),
            })
            .await
            .expect("Failed to sent a get_sebastian request to the gRPC server");
        let mood = response.into_inner().mood;
        assert_eq!(
            mood, "Happy",
            "Invalid Sebastian mood returned by the gRPC server"
        );

        let response = client
            .get_betsy_krabs(crabs::GetBetsyKrabsReqProto {})
            .await
            .expect_err("Failed to send a get_betsy_krabs request to the gRPC server");
        assert_eq!(
            response.message(),
            "This is not yet implemented",
            "gRPC server returned invalid status message"
        );

        cancel_token.cancel();
        server.await.expect("Server failed to gracefully shutdown");
    }
}
