//! The part of the emulator's gRPC interface the screen view uses: the messages, written out by
//! hand from `emulator/lib/emulator_controller.proto` in the SDK, and the calls.
//!
//! Only the fields used are declared; protobuf skips the others. Every call carries the token the
//! emulator wrote in its discovery file.

// Rust guideline compliant 2026-10-09

use futures::Stream;
use http::uri::PathAndQuery;
use tonic::client::Grpc;
use tonic::transport::Channel;
use tonic_prost::ProstCodec;

const SERVICE: &str = "/android.emulation.control.EmulatorController/";

/// `ImageFormat.format` for raw pixels, four bytes each.
pub(super) const RGBA8888: i32 = 1;

/// `KeyboardEvent.eventType` for a key pressed and released.
pub(super) const KEYPRESS: i32 = 2;

#[derive(Clone, PartialEq, prost::Message)]
pub(super) struct ImageFormat {
    #[prost(int32, tag = "1")]
    pub format: i32,
    #[prost(uint32, tag = "3")]
    pub width: u32,
    #[prost(uint32, tag = "4")]
    pub height: u32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub(super) struct Image {
    #[prost(message, optional, tag = "1")]
    pub format: Option<ImageFormat>,
    #[prost(bytes = "vec", tag = "4")]
    pub image: Vec<u8>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub(super) struct KeyboardEvent {
    #[prost(int32, tag = "2")]
    pub event_type: i32,
    #[prost(string, tag = "4")]
    pub key: String,
    #[prost(string, tag = "5")]
    pub text: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub(super) struct MouseEvent {
    #[prost(int32, tag = "1")]
    pub x: i32,
    #[prost(int32, tag = "2")]
    pub y: i32,
    #[prost(int32, tag = "3")]
    pub buttons: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub(super) struct InputEvent {
    #[prost(oneof = "Event", tags = "1, 3")]
    pub event: Option<Event>,
}

#[derive(Clone, PartialEq, prost::Oneof)]
pub(super) enum Event {
    #[prost(message, tag = "1")]
    Key(KeyboardEvent),
    #[prost(message, tag = "3")]
    Mouse(MouseEvent),
}

/// A connection to one emulator's gRPC server.
#[derive(Clone)]
pub(super) struct Client {
    grpc: Grpc<Channel>,
    token: String,
}

impl Client {
    pub(super) async fn connect(port: u16, token: String) -> Result<Self, String> {
        let channel = Channel::from_shared(format!("http://127.0.0.1:{port}"))
            .map_err(|e| e.to_string())?
            .connect()
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self {
            grpc: Grpc::new(channel),
            token,
        })
    }

    fn request<T>(&self, message: T) -> tonic::Request<T> {
        let mut request = tonic::Request::new(message);
        if let Ok(value) = format!("Bearer {}", self.token).parse() {
            request.metadata_mut().insert("authorization", value);
        }
        request
    }

    fn path(method: &str) -> Result<PathAndQuery, tonic::Status> {
        format!("{SERVICE}{method}")
            .parse()
            .map_err(|e| tonic::Status::internal(format!("bad method {method}: {e}")))
    }

    /// Frames of the screen as the emulator draws them, scaled to fit `format`.
    pub(super) async fn screenshots(
        &mut self,
        format: ImageFormat,
    ) -> Result<tonic::Streaming<Image>, tonic::Status> {
        self.grpc
            .ready()
            .await
            .map_err(|e| tonic::Status::unavailable(e.to_string()))?;
        let request = self.request(format);
        let response = self
            .grpc
            .server_streaming(
                request,
                Self::path("streamScreenshot")?,
                ProstCodec::default(),
            )
            .await?;
        Ok(response.into_inner())
    }

    /// Sends every event of `events` in order, until it ends or the emulator goes.
    pub(super) async fn input(
        &mut self,
        events: impl Stream<Item = InputEvent> + Send + 'static,
    ) -> Result<(), tonic::Status> {
        self.grpc
            .ready()
            .await
            .map_err(|e| tonic::Status::unavailable(e.to_string()))?;
        let request = self.request(events);
        self.grpc
            .client_streaming::<_, _, (), _>(
                request,
                Self::path("streamInputEvent")?,
                ProstCodec::default(),
            )
            .await?;
        Ok(())
    }
}
