//! One WebRTC peer connection per phone: we offer a receive-only H.264 video transceiver, the
//! phone answers and sends its camera.

use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use rtc::rtp_transceiver::rtp_sender::{RTCPFeedback, RTCRtpCodec, RTCRtpCodecParameters, RtpCodecKind};
use rtc::rtp_transceiver::{RTCRtpTransceiverDirection, RTCRtpTransceiverInit};
use tokio::sync::{mpsc, watch};
use webrtc::media_stream::track_remote::{TrackRemote, TrackRemoteEvent};
use webrtc::peer_connection::{
    MediaEngine, PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCConfigurationBuilder,
    RTCIceGatheringState, RTCPeerConnectionState, RTCSessionDescription, Registry, register_default_interceptors,
};
use webrtc::runtime::default_runtime;

use crate::video::{DecoderMsg, DecoderTx, FrameAssembler};

const MIME_TYPE_H264: &str = "video/H264";
const PLI_MIN_GAP: Duration = Duration::from_millis(500);

struct Handler {
    gathered_tx: mpsc::Sender<()>,
    decoder_tx: DecoderTx,
    want_keyframe: Arc<AtomicBool>,
    /// The session generation this connection belongs to; packets stop flowing once a newer
    /// phone connection takes over.
    generation: u64,
    current: watch::Receiver<u64>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete {
            let _ = self.gathered_tx.try_send(());
        }
    }

    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        println!("[rtc] connection state: {state}");
    }

    async fn on_track(&self, track: Arc<dyn TrackRemote>) {
        let Some(&media_ssrc) = track.ssrcs().await.first() else { return };
        println!("[rtc] receiving video from phone");
        let decoder_tx = self.decoder_tx.clone();
        let want_keyframe = self.want_keyframe.clone();
        let generation = self.generation;
        let current = self.current.clone();

        tokio::spawn(async move {
            let mut assembler = FrameAssembler::new();
            let mut last_pli: Option<Instant> = None;
            while let Some(evt) = track.poll().await {
                if *current.borrow() != generation {
                    break;
                }
                match evt {
                    TrackRemoteEvent::OnRtpPacket(pkt) => {
                        for au in assembler.push(pkt) {
                            if decoder_tx.send(DecoderMsg::AccessUnit(au)).is_err() {
                                return;
                            }
                        }
                        let wanted = assembler.need_keyframe || want_keyframe.swap(false, Ordering::Relaxed);
                        if wanted && last_pli.map_or(true, |t| t.elapsed() > PLI_MIN_GAP) {
                            last_pli = Some(Instant::now());
                            let _ = track
                                .write_rtcp(vec![Box::new(PictureLossIndication { sender_ssrc: 0, media_ssrc })])
                                .await;
                        }
                    }
                    TrackRemoteEvent::OnEnded => break,
                    _ => {}
                }
            }
            println!("[rtc] video track ended");
        });
    }
}

pub struct Session {
    pub pc: Arc<dyn PeerConnection>,
}

impl Session {
    /// Builds the peer connection and returns it with a complete (non-trickle) offer.
    pub async fn create(
        bind_ip: IpAddr,
        decoder_tx: DecoderTx,
        want_keyframe: Arc<AtomicBool>,
        generation: u64,
        current: watch::Receiver<u64>,
    ) -> anyhow::Result<(Self, RTCSessionDescription)> {
        let mut media_engine = MediaEngine::default();
        let feedback = |typ: &str, parameter: &str| RTCPFeedback { typ: typ.into(), parameter: parameter.into() };
        media_engine.register_codec(
            RTCRtpCodecParameters {
                rtp_codec: RTCRtpCodec {
                    mime_type: MIME_TYPE_H264.to_owned(),
                    clock_rate: 90000,
                    channels: 0,
                    // Constrained Baseline: what iOS hardware-encodes and openh264 decodes.
                    sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f".to_owned(),
                    rtcp_feedback: vec![feedback("nack", ""), feedback("nack", "pli"), feedback("ccm", "fir")],
                },
                payload_type: 102,
                ..Default::default()
            },
            RtpCodecKind::Video,
        )?;
        let registry = register_default_interceptors(Registry::new(), &mut media_engine)?;

        let (gathered_tx, mut gathered_rx) = mpsc::channel(1);
        let handler = Arc::new(Handler { gathered_tx, decoder_tx, want_keyframe, generation, current });

        let pc = PeerConnectionBuilder::new()
            // LAN only: the phone reaches our host candidate directly, no STUN needed.
            .with_configuration(RTCConfigurationBuilder::new().build())
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .with_handler(handler as Arc<dyn PeerConnectionEventHandler>)
            .with_runtime(default_runtime().ok_or_else(|| anyhow::anyhow!("no webrtc runtime"))?)
            .with_udp_addrs(vec![format!("{bind_ip}:0")])
            .build()
            .await?;
        let pc: Arc<dyn PeerConnection> = Arc::new(pc);

        pc.add_transceiver_from_kind(
            RtpCodecKind::Video,
            Some(RTCRtpTransceiverInit { direction: RTCRtpTransceiverDirection::Recvonly, ..Default::default() }),
        )
        .await?;

        let offer = pc.create_offer(None).await?;
        pc.set_local_description(offer).await?;
        let _ = tokio::time::timeout(Duration::from_secs(3), gathered_rx.recv()).await;
        let offer = pc.local_description().await.ok_or_else(|| anyhow::anyhow!("no local description"))?;

        Ok((Self { pc }, offer))
    }
}
