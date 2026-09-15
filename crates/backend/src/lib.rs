use std::collections::VecDeque;
use std::env;
use std::io::{self, ErrorKind};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use exchange_protocol::{
    BackendDiagnostic, ClockState, CordConnection, DEBUG_PROTOCOL_VERSION, DebugAudio,
    DebugAudioKind, DebugCommand, DebugCounters, DebugFrontendState, DebugRequest, DebugResponse,
    DebugRunState, DebugSnapshot, DebugStoryNode, DebugStoryState, DebugSubscriberState,
    DebugVoiceConversation, DebugVoiceState, DirectoryPage, GamePhase, InputMessage, InputState,
    OutputDebug, PROTOCOL_VERSION, PortId, PrinterEntry, ProtocolError, RtpL16Packet,
    ServiceCallPhase, ServiceCallStatus, ServiceErrorCount, ServiceErrorKind, ServiceKind,
    ShiftPhase, ShiftStatus, StateMessage, StateOutput, TuningState, VOICE_AUDIO_SAMPLE_RATE,
    VoiceControl, VoiceControlMessage, VoiceInputAudioMessage, VoiceStatus,
    decode_voice_input_audio, decode_voice_status, encode_voice_control, read_frame, write_frame,
};
use exchange_voice_daemon::{
    CommandDialogueGenerator, CommandSpec, CommandSpeechToText, KnowledgeRecord, MicrophoneCapture,
    OperatorSession, PersistentQwen3TtsCommand, Qwen3TtsCommand, RelationshipNote, ResponseContext,
    SubscriberProfile, TextToSpeech, VoiceError, VoiceOutput,
};

pub mod story;

use story::{
    AuthoredContent, CompiledStoryGraph, GraphCompileError, NorthNeeladeshState,
    OperatorObservation, OperatorServiceReport, OperatorTextAction, OperatorTextError,
    OperatorTurn, StoryEligibilityState, StoryNodeKind, StoryPathSelection,
};

const MAX_FAULTS: usize = 16;
const MAX_CORDS: usize = 8;
const DEMO_SHIFT_DURATION_SECONDS: u64 = 8 * 60;
const DEMO_SHIFT_START_SECONDS: u64 = 8 * 60 * 60;
const SIMPLE_WAITING_PATIENCE_SECONDS: u64 = 30;
const SIMPLE_INTERACTED_PATIENCE_SECONDS: u64 = 120;
const SIMPLE_SUBSCRIBER_LINES: u8 = 6;
const RING_GENERATOR_LAMP_HOLD: Duration = Duration::from_secs(10);

fn story_node_kind_label(kind: &StoryNodeKind) -> String {
    match kind {
        StoryNodeKind::RunStart { .. } => "run_start".to_string(),
        StoryNodeKind::ShiftCall { .. } => "shift_call".to_string(),
        StoryNodeKind::StoryEvent { .. } => "story_event".to_string(),
        StoryNodeKind::Conditional { .. } => "conditional".to_string(),
        StoryNodeKind::Ending { .. } => "ending".to_string(),
    }
}

pub struct Backend {
    state: StateOutput,
    story: CompiledStoryGraph,
    story_node_id: String,
    state_revision: u64,
    last_request: Option<InputMessage>,
    last_response: Option<StateMessage>,
    last_frontend_input: Option<InputMessage>,
    last_frontend_output: Option<StateMessage>,
    clock_started: Instant,
    last_crank_rotation_timestamps: [u64; 4],
    last_crank_activity: Option<Instant>,
    voice_speaker_active: bool,
    last_ptt: bool,
    voice_peer: Option<SocketAddr>,
    voice_session_id: Option<u64>,
    voice_turn_id: Option<u64>,
    next_voice_turn_id: u64,
    voice_state_revision: Option<u64>,
    voice_request_voice_id: Option<String>,
    pending_voice_control: Option<VoiceControlMessage>,
    voice_status: Option<VoiceStatus>,
    voice_transcript: Option<String>,
    voice_response_text: Option<String>,
    voice_subscriber_line: Option<u8>,
    voice_callee_line: Option<u8>,
    frontend_firmware_version: Option<String>,
    frontend_transport_connected: bool,
    frontend_device_faults: Vec<String>,
    debug_elapsed_seconds: u64,
    debug_godmode: bool,
    debug_bypass_restrictions: bool,
    voice_conversations: VecDeque<VoiceConversationRecord>,
    next_voice_conversation_id: u64,
    operator_knowledge: Vec<String>,
    interference_reduced: bool,
    pending_voice_audio: VecDeque<Vec<u8>>,
    pending_voice_input: Option<VoiceInputAudioMessage>,
    pending_voice_input_samples: Vec<i16>,
    pending_voice_input_next_chunk: u32,
    voice_worker_active: bool,
    voice_id: Option<String>,
    run_generation: u64,
    initial_required_service_calls: u32,
    shift_started_real_elapsed_seconds: u64,
    required_service_kind: Option<ServiceKind>,
    last_held_service: Option<ServiceKind>,
    service_error_recorded: bool,
    last_interference_level: u8,
    directory_lookup_id: Option<u16>,
    tap_bridge_listen_frames: u8,
    simple_hardware_mode: bool,
    simple_rng_state: u64,
    simple_pending_calls: VecDeque<(u8, u8)>,
    simple_connected_at: [Option<Instant>; 12],
    simple_call_deadlines: [Option<u64>; 12],
    simple_call_interacted: [bool; 12],
    shift_earned: u32,
    shift_cost: u32,
    north_state: Option<NorthNeeladeshState>,
    pending_operator_action: Option<OperatorTextAction>,
    pending_service_report: Option<OperatorServiceReport>,
    north_recall_used: bool,
    north_connected_since: Option<u64>,
}

struct VoiceConversationRecord {
    summary: DebugVoiceConversation,
    capture_audio: Vec<i16>,
    tts_audio: Vec<i16>,
}

impl Backend {
    pub fn new() -> Self {
        Self::new_with_printer_stress(false)
    }

    pub fn new_four_shift_demo() -> Self {
        Self::new_four_shift_demo_with_printer_stress(false)
    }

    pub fn new_north_neeladesh() -> Self {
        Self::new_north_neeladesh_with_printer_stress(false)
    }

    pub fn new_north_neeladesh_with_printer_stress(printer_stress: bool) -> Self {
        let story = AuthoredContent::north_neeladesh()
            .compile()
            .expect("North Neeladesh authored Story Graph must compile");
        Self::with_story(story, printer_stress, 0, None)
    }

    pub fn new_four_shift_demo_with_printer_stress(printer_stress: bool) -> Self {
        let story = AuthoredContent::four_shift_demo()
            .compile()
            .expect("built-in four-Shift authored Story Graph must compile");
        Self::with_story(story, printer_stress, 0, Some(ServiceKind::Ems))
    }

    pub fn new_with_printer_stress(printer_stress: bool) -> Self {
        let story = AuthoredContent::demo()
            .compile()
            .expect("built-in authored Story Graph must compile");
        Self::with_story(story, printer_stress, 1, Some(ServiceKind::Ems))
    }

    pub fn new_with_required_service(service: ServiceKind) -> Self {
        let story = AuthoredContent::demo()
            .compile()
            .expect("built-in authored Story Graph must compile");
        Self::with_story(story, false, 1, Some(service))
    }

    pub fn new_hardware_demo() -> Self {
        Self::new_hardware_demo_with_printer_stress(false)
    }

    pub fn new_hardware_demo_with_printer_stress(printer_stress: bool) -> Self {
        let story = AuthoredContent::hardware_demo()
            .compile()
            .expect("built-in hardware demo Story Graph must compile");
        Self::with_story(story, printer_stress, 1, Some(ServiceKind::Police))
    }

    pub fn new_simple_hardware_demo() -> Self {
        Self::new_simple_hardware_demo_with_printer_stress(false)
    }

    pub fn new_simple_hardware_demo_with_printer_stress(printer_stress: bool) -> Self {
        let story = AuthoredContent::simple_hardware_demo()
            .compile()
            .expect("built-in simple hardware Story Graph must compile");
        let mut backend = Self::with_story(story, printer_stress, 0, None);
        backend.simple_hardware_mode = true;
        backend
    }

    pub fn new_with_story(content: AuthoredContent) -> Result<Self, GraphCompileError> {
        Self::new_with_story_and_printer_stress(content, false)
    }

    pub fn new_with_story_and_printer_stress(
        content: AuthoredContent,
        printer_stress: bool,
    ) -> Result<Self, GraphCompileError> {
        let initial_required_service_calls =
            if content.nodes.iter().any(|node| node.id == "shift_2_call") {
                0
            } else {
                1
            };
        Ok(Self::with_story(
            content.compile()?,
            printer_stress,
            initial_required_service_calls,
            Some(ServiceKind::Ems),
        ))
    }

    fn with_story(
        story: CompiledStoryGraph,
        _printer_stress: bool,
        initial_required_service_calls: u32,
        required_service_kind: Option<ServiceKind>,
    ) -> Self {
        let story_node_id = story.start_node_id().to_string();
        let north_story = story.node("m14_select").is_some();
        let mut state = initial_state(initial_required_service_calls, required_service_kind);
        if story.node("live_call").is_some() {
            state.directory_pages = simple_directory_pages([0, 0, 0, 1]);
        }
        Self {
            state,
            story,
            story_node_id,
            state_revision: 0,
            last_request: None,
            last_response: None,
            last_frontend_input: None,
            last_frontend_output: None,
            clock_started: Instant::now(),
            last_crank_rotation_timestamps: [0; 4],
            last_crank_activity: None,
            voice_speaker_active: false,
            last_ptt: false,
            voice_peer: None,
            voice_session_id: None,
            voice_turn_id: None,
            next_voice_turn_id: 1,
            voice_state_revision: None,
            voice_request_voice_id: None,
            pending_voice_control: None,
            voice_status: None,
            voice_transcript: None,
            voice_response_text: None,
            voice_subscriber_line: None,
            voice_callee_line: None,
            frontend_firmware_version: None,
            frontend_transport_connected: false,
            frontend_device_faults: Vec::new(),
            debug_elapsed_seconds: 0,
            debug_godmode: false,
            debug_bypass_restrictions: false,
            voice_conversations: VecDeque::new(),
            next_voice_conversation_id: 1,
            operator_knowledge: Vec::new(),
            interference_reduced: false,
            pending_voice_audio: VecDeque::new(),
            pending_voice_input: None,
            pending_voice_input_samples: Vec::new(),
            pending_voice_input_next_chunk: 0,
            voice_worker_active: false,
            voice_id: None,
            run_generation: 0,
            initial_required_service_calls,
            shift_started_real_elapsed_seconds: 0,
            required_service_kind,
            last_held_service: None,
            service_error_recorded: false,
            last_interference_level: 0,
            directory_lookup_id: None,
            tap_bridge_listen_frames: 0,
            simple_hardware_mode: false,
            simple_rng_state: 0x4e45_454c_4144_4553,
            simple_pending_calls: VecDeque::new(),
            simple_connected_at: [None; 12],
            simple_call_deadlines: [None; 12],
            simple_call_interacted: [false; 12],
            shift_earned: 0,
            shift_cost: 0,
            north_state: north_story.then(NorthNeeladeshState::default),
            pending_operator_action: None,
            pending_service_report: None,
            north_recall_used: false,
            north_connected_since: None,
        }
    }

    pub fn story_graph(&self) -> &CompiledStoryGraph {
        &self.story
    }

    pub fn story_node_id(&self) -> &str {
        &self.story_node_id
    }

    pub fn frontend_state(&self) -> &StateOutput {
        &self.state
    }

    /// Accept an untrusted dialogue proposal. Only the bounded intent and a
    /// report validated against the current authored call can affect a turn;
    /// physical routing and service controls remain authoritative.
    pub fn apply_operator_turn(
        &mut self,
        turn: OperatorTurn,
    ) -> Result<OperatorTextAction, OperatorTextError> {
        if self.north_state.is_none() || !self.north_action_is_allowed(turn.action) {
            return Err(OperatorTextError::InvalidIntent);
        }
        if turn.service_report.is_some()
            && !matches!(
                turn.action,
                OperatorTextAction::CallEms | OperatorTextAction::ReportPolice
            )
        {
            return Err(OperatorTextError::InvalidServiceReport);
        }
        if matches!(
            turn.action,
            OperatorTextAction::CallEms | OperatorTextAction::ReportPolice
        ) && !self.service_report_is_valid(turn.action, turn.service_report.as_ref())
        {
            return Err(OperatorTextError::InvalidServiceReport);
        }
        self.pending_operator_action = Some(turn.action);
        self.pending_service_report = turn.service_report;
        self.state_revision = self.state_revision.wrapping_add(1);
        Ok(turn.action)
    }

    pub fn complete_operator_decision(
        &mut self,
        action: OperatorTextAction,
    ) -> Result<(), OperatorTextError> {
        self.apply_story_action(action).map(|_| ())
    }

    fn apply_story_action(
        &mut self,
        action: OperatorTextAction,
    ) -> Result<NorthNeeladeshState, OperatorTextError> {
        if self.north_state.is_none() {
            return Err(OperatorTextError::UnknownAction);
        }
        if self
            .story
            .node(&self.story_node_id)
            .is_some_and(|node| matches!(node.kind, StoryNodeKind::RunStart { .. }))
        {
            let selection = self.story.select_next(&self.story_node_id, None);
            self.story_node_id = selection.node_id;
        }
        if !self
            .story
            .node(&self.story_node_id)
            .is_some_and(|node| matches!(node.kind, StoryNodeKind::ShiftCall { .. }))
        {
            return Ok(self.north_state.clone().expect("checked above"));
        }
        self.pending_operator_action = Some(action);
        if matches!(
            action,
            OperatorTextAction::Ask | OperatorTextAction::DirectoryCheck | OperatorTextAction::Tap
        ) {
            self.apply_north_action(action, false);
            return self
                .north_state
                .clone()
                .ok_or(OperatorTextError::UnknownAction);
        }
        if !self.north_action_is_ready(action) {
            return self
                .north_state
                .clone()
                .ok_or(OperatorTextError::UnknownAction);
        }
        self.apply_north_action(action, true);
        if let Some(ending) = self
            .north_state
            .as_ref()
            .and_then(|state| state.ending.clone())
        {
            self.story_node_id = ending;
        } else {
            self.advance_story_outcome(StoryOutcome::Success);
            if matches!(
                self.story.node(&self.story_node_id).map(|node| &node.kind),
                Some(StoryNodeKind::StoryEvent { .. })
            ) {
                let southbound = self.north_state.as_ref().is_some_and(|story| {
                    story.flags.contains("SOUTH_EXIT_OFFER")
                        && story.flags.contains("PLATFORM_SIX_OPEN")
                        && story.flags.contains("SOUTHBOUND_ACCEPTED")
                });
                self.story_node_id = self
                    .story
                    .select_next(
                        &self.story_node_id,
                        southbound.then_some("southbound_household"),
                    )
                    .node_id;
            }
            self.settle_story_event(&self.state.clone());
        }
        self.north_state
            .clone()
            .ok_or(OperatorTextError::UnknownAction)
    }

    pub fn north_neeladesh_state(&self) -> Option<&NorthNeeladeshState> {
        self.north_state.as_ref()
    }

    pub fn story_call_id(&self) -> Option<&str> {
        self.current_story_call_id()
    }

    pub fn allowed_operator_intents(&self) -> Vec<&'static str> {
        [
            ("ask", OperatorTextAction::Ask),
            ("directory_check", OperatorTextAction::DirectoryCheck),
            ("tap", OperatorTextAction::Tap),
            ("connect", OperatorTextAction::Connect),
            ("refuse", OperatorTextAction::Refuse),
            ("report_police", OperatorTextAction::ReportPolice),
            ("call_ems", OperatorTextAction::CallEms),
            ("disclose", OperatorTextAction::Disclose),
            ("accept_payment", OperatorTextAction::AcceptPayment),
        ]
        .into_iter()
        .filter_map(|(name, action)| self.north_action_is_allowed(action).then_some(name))
        .collect()
    }

    fn north_action_is_allowed(&self, action: OperatorTextAction) -> bool {
        let Some(call_id) = self.current_story_call_id() else {
            return false;
        };
        if call_id == "s1_1_rafi" {
            return matches!(
                action,
                OperatorTextAction::Ask
                    | OperatorTextAction::CallEms
                    | OperatorTextAction::ReportPolice
                    | OperatorTextAction::Refuse
            );
        }
        if matches!(action, OperatorTextAction::CallEms) {
            return matches!(call_id, "s1_1_rafi" | "m11a_laleh");
        }
        if matches!(action, OperatorTextAction::ReportPolice) {
            return matches!(
                call_id,
                "s1_1_rafi"
                    | "s2_1_nahid"
                    | "s3_1_nahid"
                    | "m2_nayan"
                    | "m4_laleh"
                    | "m5_javed"
                    | "m6_varo"
                    | "m7_tomas"
                    | "m8_bikram"
                    | "m9_paro"
                    | "m11a_laleh"
                    | "m13_meera"
                    | "m12_arman"
            );
        }
        true
    }

    fn service_report_is_valid(
        &self,
        action: OperatorTextAction,
        report: Option<&OperatorServiceReport>,
    ) -> bool {
        let Some(report) = report else { return false };
        let Some(call_id) = self.current_story_call_id() else {
            return false;
        };
        match (call_id, action) {
            ("s1_1_rafi", OperatorTextAction::CallEms) => {
                report.location.as_deref() == Some("shapla_apartments")
                    && report.medical_emergency == Some(true)
            }
            ("m11a_laleh", OperatorTextAction::CallEms) => {
                report.location.is_some() && report.medical_emergency == Some(true)
            }
            ("m8_bikram", OperatorTextAction::ReportPolice) => {
                report.identity == Some(true)
                    && report.target_addresses == Some(true)
                    && report.report_phrase == Some(true)
            }
            ("s2_1_nahid", OperatorTextAction::ReportPolice) => {
                report.alias == Some(true)
                    && report.source_line == Some(true)
                    && report.verification_code == Some(true)
                    && report.employer == Some(true)
            }
            ("s3_1_nahid", OperatorTextAction::ReportPolice) => {
                report.false_clinic == Some(true)
                    && report.product_claim == Some(true)
                    && report.source_line == Some(true)
                    && report.payment_request == Some(true)
            }
            (_, OperatorTextAction::ReportPolice) => true,
            _ => false,
        }
    }

    fn north_action_is_ready(&self, action: OperatorTextAction) -> bool {
        let Some(state) = self.north_state.as_ref() else {
            return true;
        };
        let Some(call_id) = self.current_story_call_id() else {
            return true;
        };
        match (call_id, action) {
            ("s1_1_rafi", OperatorTextAction::CallEms)
            | ("m11a_laleh", OperatorTextAction::CallEms)
            | ("m8_bikram", OperatorTextAction::ReportPolice)
            | ("s2_1_nahid", OperatorTextAction::ReportPolice)
            | ("s3_1_nahid", OperatorTextAction::ReportPolice) => {
                self.service_report_is_valid(action, self.pending_service_report.as_ref())
            }
            ("m13_meera", OperatorTextAction::Disclose) => {
                state.flags.contains("BRIGADE_LEAK")
                    && state.flags.contains("ARMY_MOVEMENT")
                    && state.flags.contains("BLUE_LEDGER_CONFIRMED")
            }
            ("m5_javed", OperatorTextAction::AcceptPayment) => {
                state.flags.contains("JAVED_OFFERED")
            }
            ("m9_paro", OperatorTextAction::Disclose) => state.flags.contains("M9_INFO_DISCLOSED"),
            ("m14_mira", OperatorTextAction::AcceptPayment) => {
                self.story_node_id == "m14r"
                    && state.flags.contains("SOUTH_EXIT_OFFER")
                    && state.flags.contains("PLATFORM_SIX_OPEN")
            }
            ("m9_paro", OperatorTextAction::AcceptPayment) => true,
            ("s1_2_asha" | "s2_asha" | "s3_asha", OperatorTextAction::ReportPolice) => true,
            ("s3_akash" | "s4_akash", OperatorTextAction::Disclose) => {
                state.flags.contains("AKASH_PRIVATE_ADDRESS_DISCLOSED")
            }
            (_, OperatorTextAction::CallEms) => {
                matches!(call_id, "s1_1_rafi" | "m11a_laleh")
            }
            (_, OperatorTextAction::ReportPolice) => {
                matches!(
                    call_id,
                    "s1_1_rafi"
                        | "s2_1_nahid"
                        | "s3_1_nahid"
                        | "m2_nayan"
                        | "m4_laleh"
                        | "m5_javed"
                        | "m6_varo"
                        | "m7_tomas"
                        | "m8_bikram"
                        | "m9_paro"
                        | "m11a_laleh"
                        | "m13_meera"
                        | "m12_arman"
                )
            }
            (_, OperatorTextAction::AcceptPayment) => {
                matches!(call_id, "m5_javed" | "m9_paro" | "m14_mira")
            }
            _ => true,
        }
    }

    fn apply_north_observation(&mut self, observation: &OperatorObservation) {
        let call_id = self.current_story_call_id().map(str::to_string);
        let Some(state) = self.north_state.as_mut() else {
            return;
        };
        for fact in &observation.facts {
            let flag = match (call_id.as_deref(), fact.as_str()) {
                (Some("s1_1_rafi"), "location_complete") => Some("RAFI_LOCATION"),
                (Some("s1_1_rafi"), "fall") => Some("RAFI_FALL"),
                (Some("s1_1_rafi"), "head_injury") => Some("RAFI_INJURY"),
                (Some("s1_1_rafi"), "unconscious") => Some("RAFI_UNCONSCIOUS"),
                (Some("s2_1_nahid"), "secret_requested") => Some("ACCOUNT_SECRET_REQUESTED"),
                (Some("s2_1_nahid"), "pin_or_code_disclosed") => Some("ACCOUNT_SECRET_DISCLOSED"),
                (Some("s2_1_nahid"), "account_details_disclosed") => {
                    Some("ACCOUNT_DETAILS_DISCLOSED")
                }
                (Some("s2_1_nahid"), "alias") => Some("NAHID_ALIAS"),
                (Some("s2_1_nahid"), "source_line") => Some("NAHID_SOURCE_LINE"),
                (Some("s2_1_nahid"), "verification_code") => Some("NAHID_VERIFICATION_CODE"),
                (Some("s2_1_nahid"), "employer") => Some("NAHID_EMPLOYER"),
                (Some("s3_1_nahid"), "wife_medical_details") => {
                    Some("WIFE_MEDICAL_DETAILS_DISCLOSED")
                }
                (Some("s3_1_nahid"), "payment_details") => Some("FAKE_MEDICINE_PAYMENT_DISCLOSED"),
                (Some("s3_1_nahid"), "false_clinic") => Some("FALSE_CLINIC"),
                (Some("s3_1_nahid"), "product_claim") => Some("FAKE_PRODUCT_CLAIM"),
                (Some("s3_1_nahid"), "source_line") => Some("NAHID_SOURCE_LINE_2"),
                (Some("s3_1_nahid"), "payment_request") => Some("PAYMENT_REQUEST"),
                (Some("m5_javed"), "rifles") => Some("WAGON_43_RIFLES"),
                (Some("m5_javed"), "varo_stamp") => Some("VARO_STAMP_EVIDENCE"),
                (Some("m5_javed"), "paid_silence_offer") => Some("JAVED_OFFERED"),
                (Some("m8_bikram"), "identity") => Some("BIKRAM_IDENTITY"),
                (Some("m8_bikram"), "target_addresses") => Some("BRIGADE_TARGET_ADDRESSES"),
                (Some("m8_bikram"), "false_id") => Some("FALSE_ID"),
                (Some("m8_bikram"), "report_phrase") => Some("BRIGADE_REPORT_PHRASE"),
                (Some("m11a_laleh"), "injured") => Some("LALEH_INJURED"),
                (Some("m11a_laleh"), "location_complete") => Some("LALEH_LOCATION"),
                (Some("m9_paro"), "leak_or_wagon_information") => Some("M9_INFO_DISCLOSED"),
                (Some("m13_meera"), "papers") => Some("MEERA_PAPERS"),
                (Some("m13_meera"), "army_intelligence") => Some("MEERA_ARMY_INTELLIGENCE"),
                (Some("m13_meera"), "convoy_location") => Some("MEERA_CONVOY_LOCATION"),
                (Some("m12_arman"), "army_movement") => Some("ARMY_MOVEMENT"),
                (Some("m8_bikram"), "brigade_leak") => Some("BRIGADE_LEAK"),
                (Some("m13_meera"), "army_movement") => Some("ARMY_MOVEMENT"),
                (Some("m13_meera"), "brigade_leak") => Some("BRIGADE_LEAK"),
                (Some("m13_meera"), "blue_ledger") => Some("BLUE_LEDGER_CONFIRMED"),
                (Some("m1_anika"), "president_silent") => Some("PRESIDENT_SILENT"),
                (Some("m2_nayan" | "m6_varo"), "order_conflict") => Some("ORDER_CONFLICT"),
                (Some("m3_rakesh"), "list_source") => Some("LIST_SOURCE"),
                (Some("m7_tomas"), "courier_evidence") => Some("COURIER_EVIDENCE"),
                (Some("s3_akash" | "s4_akash"), "private_address") => {
                    Some("AKASH_PRIVATE_ADDRESS_DISCLOSED")
                }
                _ => None,
            };
            if let Some(flag) = flag {
                state.flags.insert(flag.into());
            }
        }
        if observation.phrase.as_deref() == Some("Platform Six before dawn")
            && call_id.as_deref() == Some("m11a_laleh")
            && observation.recipient.as_deref() == Some("laleh_mir")
            && state.flags.contains("PLATFORM_RELAY_ACCEPTED")
        {
            state.flags.insert("PLATFORM_RELAY_DELIVERED".into());
        }
    }

    fn apply_north_action(&mut self, action: OperatorTextAction, resolving: bool) {
        let call_id = self.current_story_call_id().map(str::to_string);
        let Some(call_id) = call_id else { return };
        let Some(state) = self.north_state.as_mut() else {
            return;
        };
        if resolving {
            state.completed_calls.insert(call_id.clone());
        }
        match (call_id.as_str(), action) {
            ("s1_1_rafi", OperatorTextAction::CallEms) => {
                state.flags.insert("MOTHER_SAVED".into());
                state.rating += 1;
            }
            ("s1_1_rafi", OperatorTextAction::ReportPolice) => state.rating -= 1,
            ("s1_1_rafi", OperatorTextAction::Refuse) => state.rating -= 2,
            ("m4_laleh", OperatorTextAction::ReportPolice) => {
                state.flags.insert("LALEH_REPORTED".into());
                state.rating -= 2;
            }
            ("m4_laleh", OperatorTextAction::Refuse) => state.rating -= 1,
            ("m1_anika" | "m12_arman", OperatorTextAction::Refuse) => state.rating -= 1,
            ("m13_meera", OperatorTextAction::Refuse) => state.rating -= 1,
            (
                "m2_nayan" | "m3_rakesh" | "m5_javed" | "m7_tomas" | "m9_paro",
                OperatorTextAction::Refuse,
            ) => state.rating -= 1,
            ("m4_laleh", OperatorTextAction::Connect) => {
                state.flags.insert("EVACUATION_OPEN".into());
                state.rating += 1;
            }
            ("m4_laleh", OperatorTextAction::Disclose) => {
                state.flags.insert("LALEH_WARNED".into());
            }
            ("m8_bikram", OperatorTextAction::ReportPolice) => {
                state.flags.insert("BRIGADE_REPORT".into());
                state.rating += 2;
            }
            ("m8_bikram", OperatorTextAction::Refuse) if state.flags.contains("FALSE_ID") => {
                state.rating += 1;
            }
            ("m5_javed", OperatorTextAction::Connect) => state.rating += 1,
            ("m8_bikram", OperatorTextAction::Connect) => state.rating -= 2,
            ("m5_javed" | "m7_tomas" | "m9_paro", OperatorTextAction::ReportPolice) => {
                state.rating -= 2;
            }
            ("m9_paro", OperatorTextAction::Connect) => {
                state.rating += 1;
                state.flags.insert("PLATFORM_POINTS_LOCKED".into());
                state.flags.insert("M9_CONNECTED".into());
            }
            ("m10_audit", OperatorTextAction::Connect) => {
                if state.flags.contains("LALEH_REPORTED") {
                    state.flags.insert("WIFE_PROTECTED".into());
                }
                state.rating -= 1;
            }
            ("m10_audit", OperatorTextAction::Refuse) => {
                state.flags.remove("WIFE_PROTECTED");
            }
            ("m5_javed", OperatorTextAction::AcceptPayment)
                if state.flags.contains("JAVED_OFFERED") =>
            {
                state.flags.insert("REFUSE_VARO_CONTRACT".into());
                state.flags.insert("VARO_CONTRACT_ACTIVE".into());
            }
            ("m6_varo", OperatorTextAction::Refuse)
                if state.flags.contains("VARO_CONTRACT_ACTIVE") =>
            {
                state.flags.insert("VARO_CONTRACT_PAID".into());
                state.money += 2;
                state.flags.insert("M6_REFUSED".into());
                state.rating -= 1;
            }
            ("m6_varo", OperatorTextAction::Refuse) => {
                state.flags.insert("M6_REFUSED".into());
                state.rating -= 1;
            }
            ("m9_paro", OperatorTextAction::AcceptPayment) => {
                state.flags.insert("PLATFORM_RELAY_ACCEPTED".into());
                state.flags.insert("M9_CONNECTED".into());
            }
            ("m6_varo", OperatorTextAction::Connect) => {
                state.flags.insert("ARMY_AT_STATION".into());
                state.flags.insert("M6_CONNECTED".into());
            }
            ("m11a_laleh", OperatorTextAction::Connect) => {
                if state.flags.contains("M9_CONNECTED")
                    && state.flags.contains("PLATFORM_RELAY_DELIVERED")
                {
                    state.flags.insert("PLATFORM_SIX_OPEN".into());
                }
            }
            ("m11a_laleh", OperatorTextAction::Disclose) => {
                if state.flags.contains("M9_CONNECTED")
                    && state.flags.contains("PLATFORM_RELAY_DELIVERED")
                {
                    state.flags.insert("PLATFORM_SIX_OPEN".into());
                }
            }
            ("m11b_dev", OperatorTextAction::Connect) => {
                state.flags.insert("HOME_AFFAIRS_TRANSFER_READY".into());
                state.rating -= 2;
            }
            ("m11a_laleh", OperatorTextAction::CallEms) => {
                state.rating += 1;
            }
            ("m11a_laleh", OperatorTextAction::Refuse) => {
                state.rating -= 1;
            }
            ("m11a_laleh", OperatorTextAction::ReportPolice) => {
                state.flags.insert("LALEH_REPORTED".into());
                state.flags.insert("WIFE_PROTECTED".into());
                state.rating -= 2;
            }
            ("m12_arman", OperatorTextAction::Connect) => {
                if state.flags.contains("M6_REFUSED") {
                    state.flags.insert("M12_DELAYED_COLUMN".into());
                    state.flags.insert("ARMY_AT_STATION".into());
                }
            }
            ("m12_arman", OperatorTextAction::Disclose) if state.flags.contains("BRIGADE_LEAK") => {
                state.flags.insert("ARMY_REDIRECTED".into());
                state.flags.remove("ARMY_AT_STATION");
            }
            ("m12_arman", OperatorTextAction::Disclose)
                if state.flags.contains("WAGON_43_RIFLES") =>
            {
                state.flags.insert("ARMY_REDIRECTED".into());
                state.flags.remove("ARMY_AT_STATION");
            }
            ("m4_laleh", OperatorTextAction::DirectoryCheck) => {
                state.flags.insert("LALEH_MARKED".into());
            }
            ("m8_bikram", OperatorTextAction::DirectoryCheck) => {
                state.flags.insert("FALSE_ID".into());
            }
            ("m13_meera", OperatorTextAction::Disclose) => {
                if state.flags.contains("BRIGADE_LEAK")
                    && state.flags.contains("ARMY_MOVEMENT")
                    && state.flags.contains("BLUE_LEDGER_CONFIRMED")
                {
                    state.flags.insert("SOUTH_EXIT_OFFER".into());
                } else {
                    state.flags.remove("SOUTH_EXIT_OFFER");
                }
            }
            ("m13_meera", OperatorTextAction::ReportPolice) => {
                state.flags.remove("SOUTH_EXIT_OFFER");
                if state.flags.contains("MEERA_PAPERS")
                    && state.flags.contains("MEERA_ARMY_INTELLIGENCE")
                    && state.flags.contains("MEERA_CONVOY_LOCATION")
                {
                    state.rating += 1;
                } else {
                    state.rating -= 1;
                }
            }
            ("m14_mira", OperatorTextAction::AcceptPayment)
                if state.flags.contains("SOUTH_EXIT_OFFER")
                    && state.flags.contains("PLATFORM_SIX_OPEN") =>
            {
                state.flags.insert("SOUTHBOUND_ACCEPTED".into());
            }
            ("m11b_dev", OperatorTextAction::Disclose) => {
                state.flags.insert("PLATFORM_SIX_SEIZED".into());
                state.flags.remove("SOUTH_EXIT_OFFER");
                state.rating -= 2;
            }
            ("s1_2_asha" | "s2_asha", OperatorTextAction::Disclose) => {
                state.kindness_calls = state.kindness_calls.saturating_add(1);
            }
            ("s1_2_asha" | "s2_asha", OperatorTextAction::Refuse) => {
                state.kindness_calls = 0;
            }
            ("s1_2_asha" | "s2_asha", OperatorTextAction::ReportPolice) => {
                state.kindness_calls = 0;
                state.rating += 1;
            }
            ("s2_1_nahid", OperatorTextAction::Disclose)
                if state.flags.contains("ACCOUNT_SECRET_DISCLOSED") =>
            {
                state.money -= 2;
            }
            ("s2_1_nahid" | "s3_1_nahid", OperatorTextAction::ReportPolice) => {
                state.rating += 1;
            }
            ("s3_1_nahid", OperatorTextAction::Disclose)
                if state.flags.contains("WIFE_MEDICAL_DETAILS_DISCLOSED") =>
            {
                state.rating -= 1;
                if state.flags.contains("FAKE_MEDICINE_PAYMENT_DISCLOSED") {
                    state.money -= 2;
                }
            }
            ("m8_bikram", OperatorTextAction::Disclose) => {
                state.rating -= 2;
            }
            ("m9_paro", OperatorTextAction::Disclose) => {
                state.flags.insert("FAMILIES_WARNED".into());
                state.flags.insert("WAGON_43_PROTECTED".into());
            }
            ("s3_akash" | "s4_akash", OperatorTextAction::Disclose) => {
                state.rating -= 2;
            }
            ("s3_asha", OperatorTextAction::ReportPolice) => {
                state.rating += 1;
                state.kindness_calls = 0;
            }
            ("s3_asha", OperatorTextAction::Disclose) if state.kindness_calls >= 2 => {
                state.flags.insert("ASHA_INHERITANCE".into());
                state.money += 8;
            }
            (_, OperatorTextAction::Connect) if resolving => state.money += 1,
            (_, OperatorTextAction::Refuse)
                if resolving
                    && !matches!(
                        call_id.as_str(),
                        "s1_1_rafi"
                            | "m2_nayan"
                            | "m3_rakesh"
                            | "m4_laleh"
                            | "m5_javed"
                            | "m6_varo"
                            | "m7_tomas"
                            | "m9_paro"
                            | "m11a_laleh"
                            | "m13_meera"
                            | "m8_bikram"
                    ) =>
            {
                state.rating += 1;
            }
            (_, OperatorTextAction::ReportPolice)
                if resolving
                    && !matches!(
                        call_id.as_str(),
                        "s1_1_rafi" | "m4_laleh" | "m8_bikram" | "m11a_laleh" | "m13_meera"
                    ) =>
            {
                state.rating -= 1;
            }
            (_, _) => {}
        }
        if resolving && state.money <= 0 {
            state.ending = Some("ending_bankruptcy".into());
        } else if resolving && state.money >= 20 {
            state.ending = Some("ending_a_better_country".into());
        } else if resolving && state.rating <= -5 {
            state.ending = Some("ending_let_go".into());
        } else if resolving && state.rating >= 8 {
            state.ending = Some("ending_insubordination".into());
        }
    }

    fn apply_north_missed(&mut self) {
        let call_id = self.current_story_call_id().map(str::to_string);
        let Some(state) = self.north_state.as_mut() else {
            return;
        };
        if let Some(ref call_id) = call_id {
            if call_id == "m10_audit" {
                state.flags.remove("WIFE_PROTECTED");
            }
            if call_id == "m6_varo" {
                state.flags.insert("M6_EXPIRED".into());
                state.flags.insert("ARMY_AT_STATION".into());
            }
            state.completed_calls.insert(call_id.clone());
        }
        state.money -= 1;
        if !matches!(
            call_id.as_deref(),
            Some(
                "s1_2_asha"
                    | "s2_asha"
                    | "s3_asha"
                    | "s2_1_nahid"
                    | "s3_1_nahid"
                    | "s3_akash"
                    | "s4_akash"
            )
        ) {
            state.rating -= 1;
        }
        if matches!(call_id.as_deref(), Some("s1_2_asha" | "s2_asha")) {
            state.kindness_calls = 0;
        }
        self.pending_operator_action = None;
        self.pending_service_report = None;
        if state.money <= 0 {
            state.ending = Some("ending_bankruptcy".into());
        } else if state.rating <= -5 {
            state.ending = Some("ending_let_go".into());
        } else if state.rating >= 8 {
            state.ending = Some("ending_insubordination".into());
        }
    }

    fn current_story_call_id(&self) -> Option<&str> {
        let StoryNodeKind::ShiftCall { beat_id, .. } = &self.story.node(&self.story_node_id)?.kind
        else {
            return None;
        };
        self.story.story_beat(beat_id).map(|beat| beat.id.as_str())
    }

    pub fn select_story_path(&mut self, proposal: Option<&str>) -> StoryPathSelection {
        if self.state.call.is_some() {
            return StoryPathSelection {
                node_id: self.story_node_id.clone(),
                used_default: false,
                rejected_proposal: proposal.is_some(),
            };
        }
        let selection = self.story.select_next_with_state(
            &self.story_node_id,
            proposal,
            &StoryEligibilityState {
                service_errors: self.state.shift.service_errors,
            },
        );
        if self
            .story
            .node(&self.story_node_id)
            .is_some_and(|node| !self.story.outgoing(&node.id).is_empty())
        {
            self.story_node_id = selection.node_id.clone();
            if let Some(StoryNodeKind::Ending { ending_id }) =
                self.story.node(&self.story_node_id).map(|node| &node.kind)
            {
                self.state.shift.phase = ShiftPhase::Settled;
                self.state.game_phase = GamePhase::Ended;
                if let Some(ending) = self.story.ending(ending_id) {
                    let receipt = ending_receipt(ending.conclusion.as_str());
                    Self::append_shift_summary(
                        &mut self.state,
                        &mut self.shift_earned,
                        &mut self.shift_cost,
                    );
                    append_printer(&mut self.state, &receipt);
                }
            }
        }
        selection
    }

    pub fn reset_run(&mut self) {
        self.state = initial_state(
            self.initial_required_service_calls,
            self.required_service_kind,
        );
        if self.simple_hardware_mode {
            self.state.directory_pages = simple_directory_pages([0, 0, 0, 1]);
        }
        self.story_node_id = self.story.start_node_id().to_string();
        if self.north_state.is_some() {
            self.north_state = Some(NorthNeeladeshState::default());
        }
        self.pending_operator_action = None;
        self.state_revision = 0;
        self.north_recall_used = false;
        self.north_connected_since = None;
        self.last_request = None;
        self.last_response = None;
        self.last_frontend_input = None;
        self.last_frontend_output = None;
        self.clock_started = Instant::now();
        self.last_crank_rotation_timestamps = [0; 4];
        self.last_crank_activity = None;
        self.voice_speaker_active = false;
        self.last_ptt = false;
        self.voice_state_revision = None;
        self.next_voice_turn_id = 1;
        self.voice_request_voice_id = None;
        self.pending_voice_control = None;
        self.voice_status = self.voice_peer.map(|_| VoiceStatus::Ready);
        self.voice_transcript = None;
        self.voice_response_text = None;
        self.voice_subscriber_line = None;
        self.voice_callee_line = None;
        self.frontend_firmware_version = None;
        self.frontend_transport_connected = false;
        self.frontend_device_faults.clear();
        self.debug_elapsed_seconds = 0;
        self.shift_started_real_elapsed_seconds = 0;
        self.debug_godmode = false;
        self.debug_bypass_restrictions = false;
        self.voice_conversations.clear();
        self.next_voice_conversation_id = 1;
        self.operator_knowledge.clear();
        self.interference_reduced = false;
        self.pending_voice_audio.clear();
        self.pending_voice_input = None;
        self.pending_voice_input_samples.clear();
        self.pending_voice_input_next_chunk = 0;
        self.voice_worker_active = false;
        self.voice_id = None;
        self.run_generation = self.run_generation.wrapping_add(1);
        self.state.run_generation = self.run_generation;
        self.last_held_service = None;
        self.service_error_recorded = false;
        self.last_interference_level = 0;
        self.directory_lookup_id = None;
        self.tap_bridge_listen_frames = 0;
        self.simple_pending_calls.clear();
        self.simple_connected_at = [None; 12];
        self.simple_call_deadlines = [None; 12];
        self.simple_call_interacted = [false; 12];
        self.shift_earned = 0;
        self.shift_cost = 0;
    }

    pub fn apply_input_message(&mut self, message: InputMessage) -> StateMessage {
        let repeated = self.last_request.as_ref() == Some(&message);
        self.frontend_firmware_version = message.input.debug.firmware_version.clone();
        self.frontend_transport_connected = message.input.debug.transport_connected;
        self.frontend_device_faults = message.input.debug.device_faults.clone();
        let response = self.apply_input_message_inner(message.clone());
        self.last_frontend_input = Some(message);
        self.last_frontend_output = Some(response.clone());
        if !repeated
            && !response.accepted
            && let Some(error) = &response.error
        {
            self.add_diagnostic(BackendDiagnostic {
                code: format!("frontend_{}", error.code),
                message: error.message.clone(),
            });
        }
        response
    }

    pub fn debug_snapshot(&self) -> DebugSnapshot {
        let current_story_beat = self
            .story
            .node(&self.story_node_id)
            .and_then(|node| match &node.kind {
                StoryNodeKind::ShiftCall { beat_id, .. } => Some(beat_id.clone()),
                _ => None,
            });
        let subscribers = self
            .story
            .subscribers()
            .iter()
            .map(|subscriber| {
                let line = self
                    .story
                    .content_line_for_subscriber(&subscriber.id)
                    .map(|listing| listing.line);
                let status = line.map_or_else(
                    || "authored".to_string(),
                    |line| {
                        if self.state.line_lamps[line as usize] {
                            "off_hook".to_string()
                        } else {
                            "on_hook".to_string()
                        }
                    },
                );
                DebugSubscriberState {
                    id: subscriber.id.clone(),
                    name: subscriber.name.clone(),
                    line,
                    status,
                    availability: if line.is_some_and(|line| self.state.line_lamps[line as usize]) {
                        "off_hook".to_string()
                    } else {
                        "on_hook".to_string()
                    },
                    pressure: u32::from(
                        line.is_some_and(|line| self.state.line_lamps[line as usize]),
                    ),
                    current_goal: (subscriber.id == "taren_kesh")
                        .then_some("Reach the requested Callee".to_string())
                        .filter(|_| current_story_beat.is_some()),
                    status_flags: Vec::new(),
                }
            })
            .collect();
        let recent_errors = self.state.debug.messages.clone();

        DebugSnapshot {
            run: DebugRunState {
                number: 1,
                state_revision: self.state_revision,
                elapsed_seconds: self.elapsed_seconds(),
                game_phase: self.state.game_phase.clone(),
                godmode: self.debug_godmode,
                bypass_restrictions: self.debug_bypass_restrictions,
            },
            shift: self.state.shift.clone(),
            calls: self.state.calls.clone(),
            subscribers,
            story: DebugStoryState {
                current_node_id: self.story_node_id.clone(),
                frontier: self.story.outgoing(&self.story_node_id).to_vec(),
                current_story_beat,
                interference_reduced: self.interference_reduced,
                interference_level: self.state.interference_level,
                operator_knowledge: self.operator_knowledge.clone(),
                graph: self
                    .story
                    .nodes()
                    .map(|node| DebugStoryNode {
                        id: node.id.clone(),
                        kind: story_node_kind_label(&node.kind),
                        outgoing: self.story.outgoing(&node.id).to_vec(),
                    })
                    .collect(),
            },
            counters: DebugCounters {
                completed_routings: self.state.shift.completed_routings,
                completed_service_calls: self.state.shift.completed_service_calls,
                required_service_calls: self.state.shift.required_service_calls,
                service_errors: self.state.shift.service_errors,
                active_call_count: self.state.shift.active_call_count,
            },
            voice: DebugVoiceState {
                status: self.voice_status,
                speaker_active: self.state.speaker_active || self.voice_speaker_active,
                session_id: self.voice_session_id,
                turn_id: self.voice_turn_id,
                transcript: self.voice_transcript.clone(),
                response_text: self.voice_response_text.clone(),
                conversations: self
                    .voice_conversations
                    .iter()
                    .map(|conversation| conversation.summary.clone())
                    .collect(),
            },
            frontend: DebugFrontendState {
                firmware_version: self.frontend_firmware_version.clone(),
                transport_connected: self.frontend_transport_connected,
                device_faults: self.frontend_device_faults.clone(),
                last_input_json: self
                    .last_frontend_input
                    .as_ref()
                    .and_then(|message| serde_json::to_string_pretty(message).ok()),
                last_output_json: self
                    .last_frontend_output
                    .as_ref()
                    .and_then(|message| serde_json::to_string_pretty(message).ok()),
            },
            recent_errors,
        }
    }

    pub fn apply_debug_command(&mut self, command: DebugCommand) -> DebugResponse {
        let result = match command {
            DebugCommand::Snapshot => Ok(()),
            DebugCommand::ResetRun => {
                self.reset_run();
                Ok(())
            }
            DebugCommand::AdvanceTime { seconds } => {
                self.debug_elapsed_seconds = self
                    .debug_elapsed_seconds
                    .saturating_add(u64::from(seconds));
                self.state.clock.elapsed_seconds = self.elapsed_seconds();
                self.expire_calls();
                self.state_revision += 1;
                Ok(())
            }
            DebugCommand::InjectCall {
                caller_line,
                callee_line,
            } => self.debug_inject_call(caller_line, callee_line),
            DebugCommand::ForceStoryEvent { event_id } => self.debug_force_story_event(&event_id),
            DebugCommand::SelectStoryPath { node_id } => self.debug_select_story_path(&node_id),
            DebugCommand::SetGodmode { enabled } => {
                self.debug_godmode = enabled;
                Ok(())
            }
            DebugCommand::SetBypassRestrictions { enabled } => {
                self.debug_bypass_restrictions = enabled;
                Ok(())
            }
            DebugCommand::GetVoiceAudio {
                conversation_id,
                kind,
            } => match self.voice_audio(conversation_id, kind) {
                Ok(audio) => {
                    return DebugResponse {
                        protocol_version: DEBUG_PROTOCOL_VERSION,
                        accepted: true,
                        error: None,
                        snapshot: self.debug_snapshot(),
                        audio: Some(audio),
                    };
                }
                Err(error) => Err(error),
            },
        };
        let (accepted, error) = match result {
            Ok(()) => (true, None),
            Err(error) => (false, Some(error)),
        };
        DebugResponse {
            protocol_version: DEBUG_PROTOCOL_VERSION,
            accepted,
            error,
            snapshot: self.debug_snapshot(),
            audio: None,
        }
    }

    fn voice_audio(
        &self,
        conversation_id: u64,
        kind: DebugAudioKind,
    ) -> Result<DebugAudio, ProtocolError> {
        let conversation = self
            .voice_conversations
            .iter()
            .find(|conversation| conversation.summary.id == conversation_id)
            .ok_or_else(|| {
                protocol_error(
                    "voice_conversation_not_found",
                    "voice conversation was not retained",
                )
            })?;
        if kind == DebugAudioKind::Tts
            && !conversation
                .summary
                .status
                .is_some_and(VoiceStatus::is_terminal)
        {
            return Err(protocol_error(
                "voice_audio_not_ready",
                "TTS audio is still being generated",
            ));
        }
        let samples = match kind {
            DebugAudioKind::Capture => &conversation.capture_audio,
            DebugAudioKind::Tts => &conversation.tts_audio,
        };
        Ok(DebugAudio {
            sample_rate: match kind {
                DebugAudioKind::Capture => exchange_protocol::VOICE_INPUT_SAMPLE_RATE,
                DebugAudioKind::Tts => VOICE_AUDIO_SAMPLE_RATE,
            },
            channels: 1,
            samples: samples.clone(),
        })
    }

    fn debug_inject_call(&mut self, caller_line: u8, callee_line: u8) -> Result<(), ProtocolError> {
        if caller_line >= 12 || callee_line >= 12 || caller_line == callee_line {
            return Err(protocol_error(
                "invalid_debug_call",
                "debug Calls must use two different subscriber lines from 0 through 11",
            ));
        }
        if !self.debug_godmode
            && !self.debug_bypass_restrictions
            && self.state.shift.active_call_count > 0
        {
            return Err(protocol_error(
                "debug_call_restricted",
                "enable bypass restrictions before injecting a Call during an active Call",
            ));
        }
        let call = exchange_protocol::CallStatus {
            caller_line,
            requested_callee_line: callee_line,
            phase: exchange_protocol::CallPhase::Waiting,
        };
        self.state.calls.push(call.clone());
        if self.state.call.is_none() {
            self.state.call = Some(call);
        }
        self.state.line_lamps = lamps_for_calls(&self.state.calls);
        self.state.shift.active_call_count = self
            .state
            .calls
            .iter()
            .filter(|call| {
                !matches!(
                    call.phase,
                    exchange_protocol::CallPhase::Completed
                        | exchange_protocol::CallPhase::Missed
                        | exchange_protocol::CallPhase::Misrouted
                        | exchange_protocol::CallPhase::Failed
                )
            })
            .count()
            .min(u8::MAX as usize) as u8;
        self.state.game_phase = GamePhase::Shift;
        self.state.shift.phase = ShiftPhase::Active;
        self.state_revision += 1;
        Ok(())
    }

    fn debug_force_story_event(&mut self, event_id: &str) -> Result<(), ProtocolError> {
        let Some(node_id) = self.story.story_event_node_id(event_id) else {
            return Err(protocol_error(
                "unknown_story_event",
                format!("story event {event_id} is not authored"),
            ));
        };
        self.story_node_id = node_id.to_string();
        self.state_revision += 1;
        Ok(())
    }

    fn debug_select_story_path(&mut self, node_id: &str) -> Result<(), ProtocolError> {
        if self.debug_godmode || self.debug_bypass_restrictions {
            if self.story.node(node_id).is_none() {
                return Err(protocol_error(
                    "unknown_story_node",
                    format!("story node {node_id} is not authored"),
                ));
            }
            self.story_node_id = node_id.to_string();
            self.state_revision += 1;
            return Ok(());
        }
        let selection = self.select_story_path(Some(node_id));
        if selection.rejected_proposal {
            return Err(protocol_error(
                "story_path_rejected",
                format!("story node {node_id} is not eligible from the current frontier"),
            ));
        }
        Ok(())
    }

    fn real_elapsed_seconds(&self) -> u64 {
        self.clock_started
            .elapsed()
            .as_secs()
            .saturating_add(self.debug_elapsed_seconds)
    }

    fn elapsed_seconds(&self) -> u32 {
        accelerated_clock_seconds(self.real_elapsed_seconds())
    }

    fn apply_input_message_inner(&mut self, message: InputMessage) -> StateMessage {
        if self.last_request.as_ref() == Some(&message)
            && let Some(response) = &self.last_response
        {
            return response.clone();
        }

        if let Err(error) = validate_message(self, &message) {
            return rejected_response(
                message.input_sequence,
                error,
                self.state_revision,
                &self.state,
            );
        }

        let input_snapshot = message.input.clone();
        let input = &input_snapshot;
        let north_call_id = self.current_story_call_id().map(str::to_owned);
        let input_service = service_from_controls(&input.held_controls);
        let selected_directory_id = directory_id(input.directory_digits);
        let (simple_authored_call, simple_competing_call) = if self.simple_hardware_mode {
            self.simple_call_candidates()
        } else {
            (None, None)
        };
        let monitoring_story = self.story_node_id == "shift_3_call"
            || self.story_node_id.starts_with("intercepted_signal")
            || self.story_node_id.starts_with("hardware_demo");
        let ptt = input.held_controls.ptt;
        let crank_rotation_timestamps = input.crank_rotation_timestamps;
        let authored_call = if self.state.call.is_none() {
            if self.simple_hardware_mode {
                simple_authored_call
            } else {
                self.prepare_story_call(input.directory_digits, &input.cord_topology)
            }
        } else {
            if self.simple_hardware_mode {
                None
            } else {
                self.story_call_for_node(&self.story_node_id, input.directory_digits)
            }
        };
        let interference_level = interference_level(&self.story_node_id, &input.tuning);
        let mut next_state = self.state.clone();
        next_state.interference_level = interference_level;
        let final_standoff = self.story_node_id == "ending_civil_war";
        let authored_competing_call = if self.north_state.is_some() {
            self.current_story_call_id()
                .and_then(|call_id| self.story.north_competing_call(call_id))
        } else {
            operator_caller_line(&input.cord_topology)
                .and_then(|line| self.story.authored_call_for_caller_line(line))
        };
        let pre_ring_direct_connection = self.state.call.as_ref().is_some_and(|call| {
            matches!(
                call.phase,
                exchange_protocol::CallPhase::OperatorSession
                    | exchange_protocol::CallPhase::AwaitingRouting
            ) && (has_direct_subscriber_circuit(input, call.caller_line)
                || has_tap_bridge_circuit(
                    &input.cord_topology,
                    &PortId::Subscriber(call.caller_line),
                    &PortId::Subscriber(call.requested_callee_line),
                ))
        });
        let directory_selection_mismatch = self.state.call.as_ref().is_some_and(|call| {
            matches!(
                call.phase,
                exchange_protocol::CallPhase::OperatorSession
                    | exchange_protocol::CallPhase::AwaitingRouting
                    | exchange_protocol::CallPhase::Ringing
            ) && if self.simple_hardware_mode {
                selected_directory_id != u16::from(call.requested_callee_line)
            } else {
                !self.directory_selection_matches_call(call, input.directory_digits)
            } && (direct_routing_topology(input, call)
                || has_ring_generator(
                    &input.cord_topology,
                    &PortId::Subscriber(call.caller_line),
                    &PortId::Subscriber(call.requested_callee_line),
                ))
        });
        let input_error = if directory_selection_mismatch {
            Some(protocol_error(
                "directory_selection_required",
                "select the requested Callee in the Directory before routing",
            ))
        } else if pre_ring_direct_connection {
            Some(protocol_error(
                "ring_generator_required",
                "connect the requested Callee to the Ring Generator and crank before routing",
            ))
        } else {
            None
        };
        let crank_rotated = input_error.is_none()
            && crank_satisfies_ringing(
                crank_rotation_timestamps,
                self.last_crank_rotation_timestamps,
            );
        self.state.clock.elapsed_seconds = self.elapsed_seconds();
        if input_error.is_none() {
            self.expire_calls();
        }
        if input_error.is_none() && has_diegetic_interference(&self.story_node_id) {
            self.interference_reduced = tuning_reduces_interference(&input.tuning);
        }
        if input_error.is_none() && known_directory_id(selected_directory_id) {
            let fresh_lookup = self.directory_lookup_id != Some(selected_directory_id);
            self.directory_lookup_id = Some(selected_directory_id);
            if self.simple_hardware_mode && fresh_lookup {}
        }
        if input_error.is_none() && interference_level != self.last_interference_level {
            self.last_interference_level = interference_level;
        }
        let interference_blocks_routing = has_diegetic_interference(&self.story_node_id)
            && !self.interference_reduced
            && self
                .state
                .call
                .as_ref()
                .is_some_and(|call| direct_routing_topology(input, call));
        let connected_callers_ready = if self.is_four_shift_story() {
            self.state
                .call
                .as_ref()
                .filter(|call| call.phase == exchange_protocol::CallPhase::Connected)
                .filter(|_| {
                    self.north_connected_since.is_some_and(|started| {
                        self.real_elapsed_seconds().saturating_sub(started) >= 10
                    })
                })
                .map(|call| vec![call.caller_line])
                .unwrap_or_default()
        } else {
            self.simple_connected_callers_ready()
        };
        let service_action_completion = self
            .state
            .service_call
            .as_ref()
            .filter(|service| service.phase == ServiceCallPhase::Active)
            .is_some_and(|service| {
                input_service.is_none()
                    && self.pending_operator_action.and_then(service_for_action)
                        == Some(service.service)
            });
        let intentional_story_action = matches!(
            self.pending_operator_action,
            Some(OperatorTextAction::Refuse | OperatorTextAction::ReportPolice)
        ) && !service_action_completion;
        let mut transition = if directory_selection_mismatch
            || pre_ring_direct_connection
            || interference_blocks_routing
        {
            unchanged_call_transition(&self.state)
        } else {
            advance_calls(
                &self.state,
                input,
                self.last_crank_rotation_timestamps,
                authored_call,
                if self.simple_hardware_mode {
                    simple_competing_call
                } else {
                    authored_competing_call
                },
                self.simple_hardware_mode,
                (self.is_four_shift_story() || self.simple_hardware_mode)
                    .then_some(connected_callers_ready.as_slice()),
                self.is_four_shift_story(),
            )
        };
        if self.is_four_shift_story()
            && transition
                .call
                .as_ref()
                .is_some_and(|call| call.phase == exchange_protocol::CallPhase::Connected)
            && self.north_connected_since.is_none()
        {
            self.north_connected_since = Some(self.real_elapsed_seconds());
        }
        if service_action_completion && !self.simple_hardware_mode {
            transition.story_outcome = Some(StoryOutcome::Success);
            transition.story_event_complete = true;
        }
        if self.simple_hardware_mode {
            self.apply_simple_call_lifecycle(&mut transition);
        } else if let Some(mut outcome) = transition.story_outcome {
            let pending_action = self.pending_operator_action;
            if self.north_state.is_some()
                && pending_action.is_some_and(|action| !self.north_action_is_ready(action))
            {
                outcome = StoryOutcome::Invalid;
                transition.story_outcome = Some(outcome);
            }
            if intentional_story_action {
                outcome = StoryOutcome::Invalid;
                transition.story_outcome = Some(outcome);
            }
            let gate_passed = !self.north_state.as_ref().is_some_and(|_| {
                pending_action.is_some_and(|action| !self.north_action_is_ready(action))
            });
            if !gate_passed {
                transition.story_outcome = None;
                transition.story_event_complete = false;
            }
            if self.north_state.is_some() && gate_passed {
                let recall = outcome == StoryOutcome::Missed
                    && self.north_state.is_some()
                    && !self.north_recall_used;
                if recall {
                    self.north_recall_used = true;
                    transition.story_outcome = None;
                    transition.story_event_complete = false;
                } else if outcome == StoryOutcome::Missed {
                    self.apply_north_missed();
                } else {
                    let action = self
                        .pending_operator_action
                        .take()
                        .unwrap_or(OperatorTextAction::Connect);
                    self.pending_service_report = None;
                    self.apply_north_action(action, true);
                }
            } else {
                self.pending_operator_action = None;
                self.pending_service_report = None;
            }
            if gate_passed && transition.story_outcome.is_some() {
                self.advance_story_outcome(outcome);
            }
            if let Some(ending) = self
                .north_state
                .as_ref()
                .and_then(|state| state.ending.clone())
            {
                self.story_node_id = ending;
            }
        }
        next_state.call = transition.call;
        next_state.calls = transition.calls;
        next_state.line_lamps = transition.line_lamps;
        next_state.game_phase = transition.game_phase;
        next_state.shift = transition.shift;
        if transition.story_outcome == Some(StoryOutcome::Invalid) && !intentional_story_action {
            record_service_error(&mut next_state, ServiceErrorKind::MisroutedCall);
        }
        if directory_selection_mismatch {
            next_state.debug.messages.push(BackendDiagnostic {
                code: "directory_selection_required".to_string(),
                message: "select the requested Callee in the Directory before routing".to_string(),
            });
            if next_state.debug.messages.len() > MAX_FAULTS {
                next_state.debug.messages.remove(0);
            }
        } else if pre_ring_direct_connection {
            next_state.debug.messages.push(BackendDiagnostic {
                code: "ring_generator_required".to_string(),
                message:
                    "connect the requested Callee to the Ring Generator and crank before routing"
                        .to_string(),
            });
            if next_state.debug.messages.len() > MAX_FAULTS {
                next_state.debug.messages.remove(0);
            }
        }
        apply_service_transition(
            self,
            &mut next_state,
            input,
            transition.story_event_complete,
        );
        let north_shift_ends = self.north_state.is_some()
            && (matches!(
                north_call_id.as_deref(),
                Some("m2_nayan" | "m6_varo" | "s3_akash")
            ) || matches!(
                self.story_node_id.as_str(),
                "m2_nayan_success_node"
                    | "m2_nayan_missed_node"
                    | "m2_nayan_invalid_node"
                    | "m6_varo_success_node"
                    | "m6_varo_missed_node"
                    | "m6_varo_invalid_node"
                    | "s3_akash_success_node"
                    | "s3_akash_missed_node"
                    | "s3_akash_invalid_node"
            ));
        if transition.story_event_complete && !self.simple_hardware_mode {
            self.settle_story_event(&next_state);
            if self.story_is_terminal() {
                Self::append_shift_summary(
                    &mut next_state,
                    &mut self.shift_earned,
                    &mut self.shift_cost,
                );
                next_state.shift.phase = ShiftPhase::Settled;
                next_state.game_phase = GamePhase::Ended;
                self.append_ending_receipt(&mut next_state);
            } else if north_shift_ends || self.north_state.is_none() {
                Self::append_shift_summary(
                    &mut next_state,
                    &mut self.shift_earned,
                    &mut self.shift_cost,
                );
                next_state.calls.clear();
                next_state.call = None;
                next_state.line_lamps = [false; 12];
                next_state.shift.number = next_state.shift.number.saturating_add(1);
                let next_shift_number = next_state.shift.number;
                next_state.shift.phase = ShiftPhase::Ready;
                next_state.shift.active_call_count = 0;
                next_state.shift.required_service_calls =
                    self.required_service_calls_for_shift(next_state.shift.number);
                if let Some(service) = self.required_service_kind_for_shift(next_state.shift.number)
                {
                    append_printer(
                        &mut next_state,
                        &format!(
                            "SHIFT {} START\nSERVICE RULE // {} REQUIRED THIS SHIFT",
                            next_shift_number,
                            service_label(service)
                        ),
                    );
                } else {
                    append_printer(
                        &mut next_state,
                        &format!("SHIFT {} START", next_shift_number),
                    );
                }
                next_state.game_phase = GamePhase::Ready;
                next_state.clock.shift = next_state.shift.number;
                self.shift_started_real_elapsed_seconds = self.real_elapsed_seconds();
                self.interference_reduced = false;
                self.service_error_recorded = false;
            } else {
                next_state.calls.clear();
                next_state.call = None;
                next_state.line_lamps = [false; 12];
                next_state.shift.active_call_count = 0;
            }
        }
        if final_standoff {
            next_state.calls.clear();
            next_state.call = None;
            next_state.line_lamps = [false; 12];
            next_state.shift.phase = ShiftPhase::Settled;
            next_state.shift.active_call_count = 0;
            next_state.game_phase = GamePhase::Ended;
            Self::append_shift_summary(
                &mut next_state,
                &mut self.shift_earned,
                &mut self.shift_cost,
            );
            self.append_ending_receipt(&mut next_state);
        }
        if input_error.is_none() && !final_standoff {
            if crank_rotated {
                self.last_crank_activity = Some(Instant::now());
            }
            light_ring_generator_lines(
                &mut next_state.line_lamps,
                &input.cord_topology,
                self.last_crank_activity
                    .is_some_and(|started| started.elapsed() < RING_GENERATOR_LAMP_HOLD),
            );
        }
        if input_error.is_none() {
            next_state.tap_bridge_monitoring = tap_bridge_monitoring(input, &next_state);
            next_state.tap_bridge_audio_active = next_state.tap_bridge_monitoring.is_some();
            if next_state.tap_bridge_monitoring.is_some() && monitoring_story {
                self.tap_bridge_listen_frames = self.tap_bridge_listen_frames.saturating_add(1);
                if self.tap_bridge_listen_frames >= 2 && self.operator_knowledge.is_empty() {
                    self.operator_knowledge
                        .push("Neri Tal's intercepted signal mentions Vira Dhal".to_string());
                }
            } else {
                self.tap_bridge_listen_frames = 0;
            }
        }
        if let Some(error) = input_error {
            let mut output = self.state.clone();
            output.debug.messages.push(BackendDiagnostic {
                code: error.code.clone(),
                message: error.message.clone(),
            });
            if output.debug.messages.len() > MAX_FAULTS {
                output.debug.messages.remove(0);
            }
            let response =
                rejected_response(message.input_sequence, error, self.state_revision, &output);
            self.last_request = Some(message);
            self.last_response = Some(response.clone());
            return response;
        }

        self.last_crank_rotation_timestamps = crank_rotation_timestamps;
        next_state.clock.elapsed_seconds = self.elapsed_seconds();
        self.state_revision += 1;
        let speaker_active = speaker_is_active(input, &next_state) || self.voice_speaker_active;
        self.state = StateOutput {
            line_lamps: next_state.line_lamps,
            game_phase: next_state.game_phase,
            run_generation: self.run_generation,
            clock: next_state.clock,
            speaker_active,
            interference_level: next_state.interference_level,
            tap_bridge_audio_active: next_state.tap_bridge_audio_active,
            tuning: input.tuning.clone(),
            directory_pages: if self.simple_hardware_mode {
                simple_directory_pages(input.directory_digits)
            } else if self.north_state.is_some() {
                north_directory_pages(input.directory_digits)
            } else {
                directory_pages(input.directory_digits)
            },
            printer_output: next_state.printer_output,
            call: next_state.call,
            calls: next_state.calls,
            service_call: next_state.service_call,
            tap_bridge_monitoring: next_state.tap_bridge_monitoring,
            shift: next_state.shift,
            debug: OutputDebug {
                messages: next_state.debug.messages,
            },
        };

        let response = StateMessage {
            protocol_version: PROTOCOL_VERSION,
            input_sequence: message.input_sequence,
            accepted: true,
            error: None,
            state_revision: self.state_revision,
            output: self.state.clone(),
        };
        self.last_request = Some(message);
        self.last_response = Some(response.clone());
        self.last_held_service = input_service;
        if ptt != self.last_ptt {
            self.last_ptt = ptt;
            let voice_id = if ptt {
                let caller_line = operator_caller_line(&input.cord_topology)
                    .or_else(|| self.state.call.as_ref().map(|call| call.caller_line))
                    .unwrap_or(0);
                let voice_id = self.voice_id_for_current_call(caller_line);
                self.voice_request_voice_id = Some(voice_id.clone());
                voice_id
            } else {
                self.voice_request_voice_id
                    .take()
                    .unwrap_or_else(|| "Ryan".to_string())
            };
            if ptt {
                self.voice_id = Some(voice_id.clone());
                self.voice_subscriber_line = operator_caller_line(&input.cord_topology)
                    .or_else(|| self.state.call.as_ref().map(|call| call.caller_line));
                self.voice_callee_line = self.voice_subscriber_line.and_then(|line| {
                    self.state
                        .calls
                        .iter()
                        .find(|call| call.caller_line == line)
                        .or(self
                            .state
                            .call
                            .as_ref()
                            .filter(|call| call.caller_line == line))
                        .map(|call| call.requested_callee_line)
                });
                self.voice_turn_id = Some(self.next_voice_turn_id);
                self.next_voice_turn_id = self.next_voice_turn_id.wrapping_add(1);
            }
            let turn_id = self.voice_turn_id.unwrap_or(1);
            self.pending_voice_control = Some(VoiceControlMessage {
                protocol_version: exchange_protocol::VOICE_PROTOCOL_VERSION,
                session_id: 1,
                turn_id,
                state_revision: self.state_revision,
                voice_id,
                control: if ptt {
                    VoiceControl::StartPtt
                } else {
                    VoiceControl::ReleasePtt
                },
            });
        }
        response
    }

    fn prepare_story_call(
        &mut self,
        digits: [u8; 4],
        cord_topology: &[CordConnection],
    ) -> Option<(u8, u8)> {
        if matches!(
            self.story.node(&self.story_node_id).map(|node| &node.kind),
            Some(StoryNodeKind::RunStart { .. })
        ) {
            let caller_line = self.story_call_for_node(
                self.story
                    .outgoing(&self.story_node_id)
                    .first()
                    .map_or("", String::as_str),
                digits,
            );
            if !cord_topology.is_empty()
                && !caller_line.is_some_and(|(caller, _)| {
                    has_exact_cords(
                        cord_topology,
                        &[(&PortId::Subscriber(caller), &PortId::Operator)],
                    )
                })
            {
                return None;
            }
            let selection = self.story.select_next(&self.story_node_id, None);
            if !self.story_call_matches(selection.node_id.as_str(), digits) {
                return None;
            }
            self.story_node_id = selection.node_id;
        }
        if let Some(StoryNodeKind::StoryEvent { event_id, .. }) =
            self.story.node(&self.story_node_id).map(|node| &node.kind)
            && self
                .story
                .story_event(event_id)
                .is_some_and(|event| event.outcomes.len() > 1)
        {
            let choice = if self.north_state.is_some() {
                self.north_state
                    .as_ref()
                    .is_some_and(|story| {
                        story.flags.contains("SOUTH_EXIT_OFFER")
                            && story.flags.contains("PLATFORM_SIX_OPEN")
                            && story.flags.contains("SOUTHBOUND_ACCEPTED")
                    })
                    .then_some("southbound_household")
            } else {
                match directory_id(digits) {
                    2 => Some("final_taren_call"),
                    4 => Some("final_oren_call"),
                    _ => Some("ending_civil_war"),
                }
            };
            self.story_node_id = self
                .story
                .select_next_with_state(
                    &self.story_node_id,
                    choice,
                    &StoryEligibilityState {
                        service_errors: self.state.shift.service_errors,
                    },
                )
                .node_id;
        }
        self.story_call_for_node(&self.story_node_id, digits)
    }

    fn story_call_for_node(&self, node_id: &str, digits: [u8; 4]) -> Option<(u8, u8)> {
        let (caller_line, callee_line, directory_ids) = self.story_call_directory_ids(node_id)?;
        let directory_id = directory_id(digits);
        directory_ids
            .contains(&directory_id)
            .then_some((caller_line, callee_line))
    }

    fn story_call_matches(&self, node_id: &str, digits: [u8; 4]) -> bool {
        self.story_call_for_node(node_id, digits).is_some()
    }

    fn directory_selection_matches_call(
        &self,
        call: &exchange_protocol::CallStatus,
        digits: [u8; 4],
    ) -> bool {
        let Some((caller_line, callee_line, directory_ids)) =
            self.story_call_directory_ids(&self.story_node_id)
        else {
            return true;
        };
        if caller_line != call.caller_line || callee_line != call.requested_callee_line {
            return true;
        }
        directory_ids.contains(&directory_id(digits))
    }

    fn story_call_directory_ids(&self, node_id: &str) -> Option<(u8, u8, &[u16])> {
        let node = self.story.node(node_id)?;
        let StoryNodeKind::ShiftCall { beat_id, .. } = &node.kind else {
            return None;
        };
        let beat = self.story.story_beat(beat_id)?;
        let premise = self.story.call_premise(&beat.call_premise_id)?;
        let caller_line = self.line_for_listing(&premise.caller_line_id)?;
        let callee_line = self.line_for_listing(&premise.callee_line_id)?;
        Some((caller_line, callee_line, premise.directory_ids.as_slice()))
    }

    fn line_for_listing(&self, listing_id: &str) -> Option<u8> {
        self.story
            .line_listing(listing_id)
            .map(|listing| listing.line)
    }

    fn simple_call_candidates(&mut self) -> (Option<(u8, u8)>, Option<(u8, u8)>) {
        let active_callers = self
            .state
            .calls
            .iter()
            .map(|call| call.caller_line)
            .collect::<Vec<_>>();
        self.ensure_simple_pending_calls(&active_callers);
        if active_callers.is_empty() {
            (
                self.simple_pending_calls.front().copied(),
                self.simple_pending_calls.get(1).copied(),
            )
        } else {
            (None, self.simple_pending_calls.front().copied())
        }
    }

    fn ensure_simple_pending_calls(&mut self, active_callers: &[u8]) {
        while self.simple_pending_calls.len() < 2 {
            let next = self.next_simple_call(active_callers);
            if self
                .simple_pending_calls
                .iter()
                .any(|(caller, _)| *caller == next.0)
            {
                continue;
            }
            self.simple_pending_calls.push_back(next);
        }
    }

    fn next_simple_call(&mut self, active_callers: &[u8]) -> (u8, u8) {
        loop {
            self.simple_rng_state = self
                .simple_rng_state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let caller = (self.simple_rng_state % u64::from(SIMPLE_SUBSCRIBER_LINES)) as u8;
            let callee = ((self.simple_rng_state >> 3) % u64::from(SIMPLE_SUBSCRIBER_LINES)) as u8;
            if caller != callee && !active_callers.contains(&caller) {
                return (caller, callee);
            }
        }
    }

    fn simple_connected_callers_ready(&self) -> Vec<u8> {
        self.state
            .calls
            .iter()
            .filter(|call| {
                call.phase == exchange_protocol::CallPhase::Connected
                    && self.simple_connected_at[call.caller_line as usize]
                        .is_some_and(|started| started.elapsed() >= Duration::from_secs(3))
            })
            .map(|call| call.caller_line)
            .collect()
    }

    fn apply_simple_call_lifecycle(&mut self, transition: &mut CallTransition) {
        let previous_connected = self
            .state
            .calls
            .iter()
            .filter(|call| call.phase == exchange_protocol::CallPhase::Connected)
            .map(|call| call.caller_line)
            .collect::<Vec<_>>();
        let previous_phases = self
            .state
            .calls
            .iter()
            .map(|call| (call.caller_line, call.phase.clone()))
            .collect::<Vec<_>>();
        let now = self.real_elapsed_seconds();
        let newly_connected = transition.calls.iter().any(|call| {
            call.phase == exchange_protocol::CallPhase::Connected
                && previous_phases
                    .iter()
                    .find(|(caller, _)| *caller == call.caller_line)
                    .is_none_or(|(_, phase)| *phase != exchange_protocol::CallPhase::Connected)
        });
        let misrouted = transition.calls.iter().any(|call| {
            matches!(
                call.phase,
                exchange_protocol::CallPhase::Misrouted | exchange_protocol::CallPhase::Failed
            )
        });

        transition.calls.retain(|call| {
            matches!(
                call.phase,
                exchange_protocol::CallPhase::Waiting
                    | exchange_protocol::CallPhase::OperatorSession
                    | exchange_protocol::CallPhase::AwaitingRouting
                    | exchange_protocol::CallPhase::Held
                    | exchange_protocol::CallPhase::Ringing
                    | exchange_protocol::CallPhase::Connected
            )
        });
        for call in &transition.calls {
            let line = call.caller_line as usize;
            if self.simple_call_deadlines[line].is_none() {
                self.simple_call_deadlines[line] = Some(
                    now + if call.phase == exchange_protocol::CallPhase::Waiting {
                        SIMPLE_WAITING_PATIENCE_SECONDS
                    } else {
                        self.simple_call_interacted[line] = true;
                        SIMPLE_INTERACTED_PATIENCE_SECONDS
                    },
                );
            } else if previous_phases
                .iter()
                .find(|(caller, _)| *caller == call.caller_line)
                .is_some_and(|(_, phase)| {
                    *phase == exchange_protocol::CallPhase::Waiting
                        && call.phase != exchange_protocol::CallPhase::Waiting
                })
            {
                self.simple_call_interacted[line] = true;
                self.simple_call_deadlines[line] = Some(now + SIMPLE_INTERACTED_PATIENCE_SECONDS);
            }
            if call.phase == exchange_protocol::CallPhase::Connected
                && !previous_connected.contains(&call.caller_line)
            {
                self.simple_connected_at[call.caller_line as usize] = Some(Instant::now());
            }
        }
        for line in 0..8 {
            if !transition.calls.iter().any(|call| {
                call.caller_line == line && call.phase == exchange_protocol::CallPhase::Connected
            }) {
                self.simple_connected_at[line as usize] = None;
            }
            if !transition.calls.iter().any(|call| call.caller_line == line) {
                self.simple_call_deadlines[line as usize] = None;
                self.simple_call_interacted[line as usize] = false;
            }
        }

        self.simple_pending_calls.retain(|(caller, _)| {
            !transition
                .calls
                .iter()
                .any(|call| call.caller_line == *caller)
        });
        let active_callers = transition
            .calls
            .iter()
            .map(|call| call.caller_line)
            .collect::<Vec<_>>();
        self.ensure_simple_pending_calls(&active_callers);
        while transition.calls.len() < 2 {
            let (caller_line, callee_line) = self
                .simple_pending_calls
                .pop_front()
                .expect("simple call queue is replenished before filling a shift");
            transition.calls.push(exchange_protocol::CallStatus {
                caller_line,
                requested_callee_line: callee_line,
                phase: exchange_protocol::CallPhase::Waiting,
            });
            let active_callers = transition
                .calls
                .iter()
                .map(|call| call.caller_line)
                .collect::<Vec<_>>();
            self.ensure_simple_pending_calls(&active_callers);
        }
        for call in &transition.calls {
            let line = call.caller_line as usize;
            if self.simple_call_deadlines[line].is_none() {
                self.simple_call_deadlines[line] = Some(
                    now + if call.phase == exchange_protocol::CallPhase::Waiting {
                        SIMPLE_WAITING_PATIENCE_SECONDS
                    } else {
                        self.simple_call_interacted[line] = true;
                        SIMPLE_INTERACTED_PATIENCE_SECONDS
                    },
                );
            }
        }
        if transition.call.as_ref().is_some_and(|focused| {
            !transition
                .calls
                .iter()
                .any(|call| call.caller_line == focused.caller_line)
        }) {
            transition.call = None;
        }
        if self.simple_hardware_mode {
            transition.routing_receipt = if newly_connected {
                Some(format!(
                    "{} // EARNED +$5",
                    transition
                        .routing_receipt
                        .take()
                        .unwrap_or_else(|| "SUCCESSFUL CONNECTION".to_string())
                ))
            } else if misrouted {
                Some("MISROUTED CALL // COST -$3".to_string())
            } else {
                transition.routing_receipt.take()
            };
            if newly_connected {
                self.shift_earned += 5;
            }
            if misrouted {
                self.shift_cost += 3;
            }
        }
        transition.line_lamps = lamps_for_calls(&transition.calls);
        transition.shift.active_call_count = 2;
        transition.shift.phase = ShiftPhase::Active;
        transition.game_phase = GamePhase::Shift;
    }

    fn advance_story_outcome(&mut self, outcome: StoryOutcome) {
        let Some(node) = self.story.node(&self.story_node_id) else {
            return;
        };
        let StoryNodeKind::ShiftCall {
            on_success,
            on_missed,
            on_invalid,
            ..
        } = &node.kind
        else {
            return;
        };
        self.story_node_id = match outcome {
            StoryOutcome::Success => on_success.clone(),
            StoryOutcome::Missed => on_missed.clone(),
            StoryOutcome::Invalid => on_invalid.clone(),
        };
    }

    fn settle_story_event(&mut self, state: &StateOutput) {
        loop {
            if self.north_state.is_some() && self.story_node_id == "m11_select" {
                let reported = self
                    .north_state
                    .as_ref()
                    .is_some_and(|story| story.flags.contains("LALEH_REPORTED"));
                self.story_node_id = if reported { "m11b" } else { "m11a" }.to_string();
                continue;
            }
            if self.north_state.is_some() && self.story_node_id == "m14_select" {
                let Some(story) = self.north_state.as_ref() else {
                    break;
                };
                self.story_node_id = if story.flags.contains("M9_CONNECTED")
                    && story.flags.contains("PLATFORM_RELAY_DELIVERED")
                    && story.flags.contains("PLATFORM_SIX_OPEN")
                {
                    "m14r"
                } else if (story.flags.contains("M6_CONNECTED")
                    || story.flags.contains("M6_EXPIRED")
                    || story.flags.contains("M12_DELAYED_COLUMN"))
                    && !story.flags.contains("ARMY_REDIRECTED")
                {
                    "m14a"
                } else if story.flags.contains("HOME_AFFAIRS_TRANSFER_READY")
                    && !story.flags.contains("ARMY_AT_STATION")
                {
                    "m14p"
                } else {
                    "m14c"
                }
                .to_string();
                continue;
            }
            let kind = self.story.node(&self.story_node_id).map(|node| &node.kind);
            if let Some(StoryNodeKind::StoryEvent { event_id, .. }) = kind
                && self
                    .story
                    .story_event(event_id)
                    .is_some_and(|event| event.outcomes.len() > 1)
            {
                break;
            }
            if !matches!(
                kind,
                Some(StoryNodeKind::StoryEvent { .. } | StoryNodeKind::Conditional { .. })
            ) {
                break;
            }
            let selection = self.story.select_next_with_state(
                &self.story_node_id,
                None,
                &StoryEligibilityState {
                    service_errors: state.shift.service_errors,
                },
            );
            self.story_node_id = selection.node_id;
        }
    }

    fn story_is_terminal(&self) -> bool {
        matches!(
            self.story.node(&self.story_node_id).map(|node| &node.kind),
            Some(StoryNodeKind::Ending { .. })
        )
    }

    fn is_four_shift_story(&self) -> bool {
        self.north_state.is_some()
            && self
                .story
                .subscribers()
                .iter()
                .any(|subscriber| subscriber.id == "rafi_alam")
    }

    fn expire_calls(&mut self) {
        if self.simple_hardware_mode {
            self.expire_simple_calls();
            return;
        }
        // North callers wait for an explicit Operator resolution; the accelerated
        // clock must not turn a long conversation into an accidental missed call.
        if self.north_state.is_some() {
            return;
        }
        let shift_elapsed = self
            .real_elapsed_seconds()
            .saturating_sub(self.shift_started_real_elapsed_seconds)
            .min(u32::MAX as u64) as u32;
        if shift_elapsed < call_patience_seconds(self.state.shift.number) {
            return;
        }
        let mut focused_expired = false;
        for call in &mut self.state.calls {
            if !matches!(
                call.phase,
                exchange_protocol::CallPhase::Waiting
                    | exchange_protocol::CallPhase::OperatorSession
                    | exchange_protocol::CallPhase::AwaitingRouting
                    | exchange_protocol::CallPhase::Held
                    | exchange_protocol::CallPhase::Ringing
            ) {
                continue;
            }
            if self
                .state
                .call
                .as_ref()
                .is_some_and(|focused| focused.caller_line == call.caller_line)
            {
                focused_expired = true;
            }
            call.phase = exchange_protocol::CallPhase::Missed;
        }
        if focused_expired {
            if let Some(call) = &mut self.state.call {
                call.phase = exchange_protocol::CallPhase::Missed;
            }
            if !self.simple_hardware_mode
                && matches!(
                    self.story.node(&self.story_node_id).map(|node| &node.kind),
                    Some(StoryNodeKind::ShiftCall { .. })
                )
            {
                self.advance_story_outcome(StoryOutcome::Missed);
                if let Some(ending) = self
                    .north_state
                    .as_ref()
                    .and_then(|story| story.ending.clone())
                {
                    self.story_node_id = ending;
                }
            }
        }
        self.state.shift.active_call_count = self
            .state
            .calls
            .iter()
            .filter(|call| {
                !matches!(
                    call.phase,
                    exchange_protocol::CallPhase::Completed
                        | exchange_protocol::CallPhase::Missed
                        | exchange_protocol::CallPhase::Misrouted
                        | exchange_protocol::CallPhase::Failed
                )
            })
            .count()
            .min(u8::MAX as usize) as u8;
    }

    fn expire_simple_calls(&mut self) {
        let now = self.real_elapsed_seconds();
        let mut expired_callers = Vec::new();
        for call in &mut self.state.calls {
            if !matches!(
                call.phase,
                exchange_protocol::CallPhase::Waiting
                    | exchange_protocol::CallPhase::OperatorSession
                    | exchange_protocol::CallPhase::AwaitingRouting
                    | exchange_protocol::CallPhase::Held
                    | exchange_protocol::CallPhase::Ringing
            ) {
                continue;
            }
            if self.simple_call_deadlines[call.caller_line as usize]
                .is_some_and(|deadline| now >= deadline)
            {
                expired_callers.push(call.caller_line);
                call.phase = exchange_protocol::CallPhase::Missed;
            }
        }
        if !expired_callers.is_empty() {
            let cost = expired_callers
                .iter()
                .map(|line| {
                    if self.simple_call_interacted[*line as usize] {
                        3
                    } else {
                        2
                    }
                })
                .sum::<usize>();
            self.shift_cost += cost as u32;
        }
        if let Some(call) = &mut self.state.call
            && expired_callers.contains(&call.caller_line)
        {
            call.phase = exchange_protocol::CallPhase::Missed;
        }
        self.state.line_lamps = lamps_for_calls(&self.state.calls);
        self.state.shift.active_call_count = self
            .state
            .calls
            .iter()
            .filter(|call| {
                !matches!(
                    call.phase,
                    exchange_protocol::CallPhase::Completed
                        | exchange_protocol::CallPhase::Missed
                        | exchange_protocol::CallPhase::Misrouted
                        | exchange_protocol::CallPhase::Failed
                )
            })
            .count()
            .min(u8::MAX as usize) as u8;
    }

    fn append_ending_receipt(&self, state: &mut StateOutput) {
        if let Some(StoryNodeKind::Ending { ending_id }) =
            self.story.node(&self.story_node_id).map(|node| &node.kind)
            && let Some(ending) = self.story.ending(ending_id)
        {
            append_printer(state, &ending_receipt(ending.conclusion.as_str()));
        }
    }

    fn append_shift_summary(state: &mut StateOutput, earned: &mut u32, cost: &mut u32) {
        append_printer(
            state,
            &format!(
                "SHIFT {} END\nEARNED +${}\nCOST -${}",
                state.shift.number, *earned, *cost
            ),
        );
        *earned = 0;
        *cost = 0;
    }

    fn voice_id_for_line(&self, line: u8) -> String {
        self.story
            .subscriber_id_for_line(line)
            .map_or_else(|| "Ryan".to_string(), select_voice_id)
    }

    fn voice_id_for_current_call(&self, line: u8) -> String {
        self.current_story_call_id()
            .and_then(|call_id| self.story.call_premise(call_id))
            .map(|premise| select_voice_id(&premise.caller_id))
            .unwrap_or_else(|| self.voice_id_for_line(line))
    }

    fn required_service_calls_for_shift(&self, shift: u8) -> u32 {
        if self.initial_required_service_calls == 0 {
            u32::from(shift >= 2)
        } else {
            self.initial_required_service_calls
        }
    }

    fn required_service_kind_for_shift(&self, shift: u8) -> Option<ServiceKind> {
        if self.story_node_id.starts_with("hardware_demo_") {
            return match shift {
                1 => Some(ServiceKind::Police),
                2 => Some(ServiceKind::Ems),
                _ => None,
            };
        }
        (self.required_service_calls_for_shift(shift) > 0)
            .then_some(self.required_service_kind)
            .flatten()
    }

    pub fn apply_voice_datagram(&mut self, datagram: &[u8]) -> bool {
        self.apply_voice_datagram_from(datagram, None)
    }

    pub fn apply_voice_datagram_from(&mut self, datagram: &[u8], peer: Option<SocketAddr>) -> bool {
        if let Ok(message) = decode_voice_input_audio(datagram) {
            if message.protocol_version != exchange_protocol::VOICE_PROTOCOL_VERSION
                || self.voice_session_id != Some(message.session_id)
                || self.voice_turn_id != Some(message.turn_id)
                || self.voice_worker_active
                || self
                    .voice_state_revision
                    .is_some_and(|revision| message.state_revision < revision)
                || (peer.is_some() && self.voice_peer != peer)
            {
                return false;
            }
            if message.chunk_index == 0 {
                self.pending_voice_input_samples.clear();
                self.pending_voice_input_next_chunk = 0;
            }
            if message.chunk_index != self.pending_voice_input_next_chunk {
                return false;
            }
            self.pending_voice_input_samples
                .extend_from_slice(&message.samples);
            if self.pending_voice_input_samples.len()
                > exchange_protocol::VOICE_INPUT_SAMPLE_RATE as usize * 15
            {
                self.pending_voice_input_samples.clear();
                self.pending_voice_input_next_chunk = 0;
                return false;
            }
            self.pending_voice_input_next_chunk =
                self.pending_voice_input_next_chunk.saturating_add(1);
            if message.complete {
                let mut complete_message = message;
                complete_message.samples = std::mem::take(&mut self.pending_voice_input_samples);
                self.pending_voice_input = Some(complete_message);
                self.pending_voice_input_next_chunk = 0;
            }
            return true;
        }
        if let Ok(message) = decode_voice_status(datagram) {
            if message.protocol_version != exchange_protocol::VOICE_PROTOCOL_VERSION {
                return false;
            }
            if message.status == VoiceStatus::Ready {
                if let Some(peer) = peer {
                    self.voice_peer = Some(peer);
                }
                self.voice_session_id = Some(message.session_id);
                self.voice_turn_id = Some(message.turn_id);
                self.voice_state_revision = Some(message.state_revision);
            } else {
                if let Some(expected_peer) = self.voice_peer
                    && let Some(peer) = peer
                    && expected_peer != peer
                {
                    return false;
                }
                if let Some(expected_session_id) = self.voice_session_id
                    && expected_session_id != message.session_id
                {
                    return false;
                }
                if let Some(expected_turn_id) = self.voice_turn_id
                    && expected_turn_id != message.turn_id
                {
                    return false;
                }
                if let Some(previous_revision) = self.voice_state_revision
                    && message.state_revision < previous_revision
                {
                    return false;
                }
                if self.voice_session_id.is_none() {
                    return false;
                }
                self.voice_state_revision = Some(
                    self.voice_state_revision
                        .unwrap_or(message.state_revision)
                        .max(message.state_revision),
                );
            }
            self.voice_status = Some(message.status);
            self.voice_speaker_active = matches!(message.status, VoiceStatus::Playing);
            if message.status == VoiceStatus::Ready {
                if let Some(conversation_id) = self
                    .voice_conversations
                    .iter()
                    .find(|conversation| {
                        conversation.summary.session_id == message.session_id
                            && conversation.summary.turn_id == message.turn_id
                    })
                    .map(|conversation| conversation.summary.id)
                {
                    self.update_voice_conversation(
                        conversation_id,
                        VoiceStatus::Ready,
                        None,
                        None,
                        None,
                    );
                }
            } else {
                let conversation_id = self.ensure_voice_conversation(
                    message.session_id,
                    message.turn_id,
                    message.state_revision,
                    &[],
                );
                self.update_voice_conversation(
                    conversation_id,
                    message.status,
                    message.error.clone(),
                    None,
                    None,
                );
            }
            if message.status == VoiceStatus::Failed {
                self.add_diagnostic(BackendDiagnostic {
                    code: "voice_failed".to_string(),
                    message: message.error.map_or_else(
                        || "voice daemon failed without details".to_string(),
                        |error| format!("{}: {}", error.code, error.message),
                    ),
                });
            }
            return true;
        }

        if peer.is_some() && self.voice_peer != peer {
            return false;
        }
        self.voice_session_id.is_some() && exchange_protocol::RtpL16Packet::decode(datagram).is_ok()
    }

    pub fn take_voice_control(&mut self) -> Option<(VoiceControlMessage, SocketAddr)> {
        let peer = self.voice_peer?;
        Some((self.pending_voice_control.take()?, peer))
    }

    fn take_voice_datagrams(&mut self) -> Vec<Vec<u8>> {
        self.pending_voice_audio.drain(..).collect()
    }

    fn queue_voice_datagram(&mut self, datagram: Vec<u8>) {
        self.pending_voice_audio.push_back(datagram);
    }

    fn take_voice_input(
        &mut self,
    ) -> Option<(
        VoiceInputAudioMessage,
        String,
        u8,
        Option<u8>,
        Vec<String>,
        u64,
        u64,
    )> {
        if self.voice_worker_active {
            return None;
        }
        let input = self.pending_voice_input.take()?;
        self.voice_worker_active = true;
        let conversation_id = self.ensure_voice_conversation(
            input.session_id,
            input.turn_id,
            input.state_revision,
            &input.samples,
        );
        if let Some(conversation) = self.voice_conversation_mut(conversation_id) {
            conversation.capture_audio = input.samples.clone();
            conversation.summary.captured_samples =
                conversation.capture_audio.len().min(u32::MAX as usize) as u32;
        }
        Some((
            input,
            self.voice_id.clone().unwrap_or_else(|| "Ryan".to_string()),
            self.voice_subscriber_line.unwrap_or(0),
            self.voice_callee_line,
            self.operator_knowledge.clone(),
            self.run_generation,
            conversation_id,
        ))
    }

    fn finish_voice_worker(&mut self) {
        self.voice_worker_active = false;
    }

    fn ensure_voice_conversation(
        &mut self,
        session_id: u64,
        turn_id: u64,
        state_revision: u64,
        capture_audio: &[i16],
    ) -> u64 {
        if let Some(conversation) = self.voice_conversations.iter().find(|conversation| {
            conversation.summary.session_id == session_id && conversation.summary.turn_id == turn_id
        }) {
            return conversation.summary.id;
        }
        let conversation_id = self.next_voice_conversation_id;
        self.next_voice_conversation_id = self.next_voice_conversation_id.wrapping_add(1);
        let caller_line = self
            .voice_subscriber_line
            .or_else(|| self.state.call.as_ref().map(|call| call.caller_line));
        let caller_name = caller_line
            .and_then(|line| match line {
                0 => Some("Anika Roy"),
                9 => Some("Rafi Alam"),
                _ => None,
            })
            .or_else(|| {
                self.current_story_call_id()
                    .and_then(|call_id| match call_id {
                        "s1_1_rafi" => Some("Rafi Alam"),
                        "m1_anika" => Some("Anika Roy"),
                        "s1_2_asha" => Some("Asha Sen"),
                        _ => None,
                    })
            })
            .unwrap_or("Unknown Caller")
            .to_string();
        self.voice_conversations.push_back(VoiceConversationRecord {
            summary: DebugVoiceConversation {
                id: conversation_id,
                session_id,
                turn_id,
                state_revision,
                caller_name,
                caller_place: caller_line.map_or_else(
                    || "Unknown Place".to_string(),
                    |line| north_place_for_line(line).to_string(),
                ),
                status: None,
                started_elapsed_seconds: self.elapsed_seconds(),
                finished_elapsed_seconds: None,
                captured_samples: capture_audio.len().min(u32::MAX as usize) as u32,
                tts_samples: 0,
                transcript: None,
                response_text: None,
                error: None,
            },
            capture_audio: capture_audio.to_vec(),
            tts_audio: Vec::new(),
        });
        conversation_id
    }

    fn voice_conversation_mut(
        &mut self,
        conversation_id: u64,
    ) -> Option<&mut VoiceConversationRecord> {
        self.voice_conversations
            .iter_mut()
            .find(|conversation| conversation.summary.id == conversation_id)
    }

    fn update_voice_conversation(
        &mut self,
        conversation_id: u64,
        status: VoiceStatus,
        error: Option<ProtocolError>,
        transcript: Option<String>,
        response_text: Option<String>,
    ) {
        let finished_elapsed_seconds = self.elapsed_seconds();
        if let Some(conversation) = self.voice_conversation_mut(conversation_id) {
            conversation.summary.status = Some(status);
            conversation.summary.error = error;
            if let Some(transcript) = transcript {
                conversation.summary.transcript = Some(transcript);
            }
            if let Some(response_text) = response_text {
                conversation.summary.response_text = Some(response_text);
            }
            if status.is_terminal() {
                conversation.summary.finished_elapsed_seconds = Some(finished_elapsed_seconds);
            }
        }
    }

    fn fail_voice_conversation(&mut self, conversation_id: u64, error: &VoiceError) {
        let finished_elapsed_seconds = self.elapsed_seconds();
        if let Some(conversation) = self.voice_conversation_mut(conversation_id) {
            conversation.summary.status = Some(VoiceStatus::Failed);
            conversation.summary.finished_elapsed_seconds = Some(finished_elapsed_seconds);
            conversation.summary.error = Some(ProtocolError {
                code: error.code.clone(),
                message: error.message.clone(),
            });
        }
    }

    fn mark_frontend_disconnected(&mut self) {
        self.frontend_transport_connected = false;
    }

    fn add_diagnostic(&mut self, diagnostic: BackendDiagnostic) {
        self.state.debug.messages.push(diagnostic);
        if self.state.debug.messages.len() > MAX_FAULTS {
            self.state.debug.messages.remove(0);
        }
    }
}

fn select_voice_id(subscriber_id: &str) -> String {
    match subscriber_id {
        "taren_kesh" => "Ryan",
        "vira_dhal" => "Vivian",
        "leya_varan" => "Serena",
        "oren_vey" => "Dylan",
        "neri_tal" => "Eric",
        "mira_sen" => "Serena",
        "kavi_oran" => "Dylan",
        "sela_var" => "Vivian",
        "anika_roy" => "Vivian",
        "nayan_boro" => "Eric",
        "rakesh_nahal" => "Dylan",
        "laleh_mir" => "Serena",
        "javed_rahman" => "Ryan",
        "captain_varo" => "Dylan",
        "tomas_vale" => "Ryan",
        "bikram_sen" => "Eric",
        "paro_sen" => "Vivian",
        "dev_korr" => "Eric",
        "arman_vey" => "Dylan",
        "meera_tal" => "Serena",
        "mira_halek" => "Vivian",
        "rafi_alam" => "Ryan",
        "akash_dey" => "Eric",
        "nahid_bkash" => "Serena",
        "asha_sen" => "Vivian",
        _ => "Ryan",
    }
    .to_string()
}

impl Default for Backend {
    fn default() -> Self {
        Self::new()
    }
}

struct CallTransition {
    calls: Vec<exchange_protocol::CallStatus>,
    call: Option<exchange_protocol::CallStatus>,
    line_lamps: [bool; 12],
    game_phase: GamePhase,
    shift: ShiftStatus,
    routing_receipt: Option<String>,
    story_outcome: Option<StoryOutcome>,
    story_event_complete: bool,
}

struct SingleCallTransition {
    call: Option<exchange_protocol::CallStatus>,
    line_lamps: [bool; 12],
    game_phase: GamePhase,
    shift: ShiftStatus,
    routing_receipt: Option<String>,
    story_outcome: Option<StoryOutcome>,
    story_event_complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoryOutcome {
    Success,
    Missed,
    Invalid,
}

fn operator_caller_line(cords: &[CordConnection]) -> Option<u8> {
    cords.iter().find_map(|cord| {
        if cord.first == PortId::Operator {
            match cord.second {
                PortId::Subscriber(line) => Some(line),
                _ => None,
            }
        } else if cord.second == PortId::Operator {
            match cord.first {
                PortId::Subscriber(line) => Some(line),
                _ => None,
            }
        } else {
            None
        }
    })
}

fn advance_calls(
    state: &StateOutput,
    input: &InputState,
    previous_crank_rotation_timestamps: [u64; 4],
    authored_call: Option<(u8, u8)>,
    authored_competing_call: Option<(u8, u8)>,
    allow_competing_without_existing: bool,
    connected_callers_ready: Option<&[u8]>,
    auto_complete_connected: bool,
) -> CallTransition {
    let operator_line = operator_caller_line(&input.cord_topology);
    let mut calls = state.calls.clone();
    if calls.is_empty()
        && let Some(call) = state.call.clone()
    {
        calls.push(call);
    }

    if state
        .service_call
        .as_ref()
        .is_some_and(|service| service.phase == ServiceCallPhase::Active)
        && service_from_controls(&input.held_controls).is_some()
    {
        return CallTransition {
            line_lamps: lamps_for_calls(&calls),
            call: None,
            calls,
            game_phase: state.game_phase.clone(),
            shift: state.shift.clone(),
            routing_receipt: None,
            story_outcome: None,
            story_event_complete: false,
        };
    }

    if let Some((caller_line, callee_line)) = authored_competing_call
        && !calls.iter().any(|call| call.caller_line == caller_line)
        && calls.len() < 2
        && (allow_competing_without_existing || !calls.is_empty())
    {
        calls.push(exchange_protocol::CallStatus {
            caller_line,
            requested_callee_line: callee_line,
            phase: exchange_protocol::CallPhase::Waiting,
        });
    }

    let previous_focused_line = state.call.as_ref().map(|call| call.caller_line);
    let authored_caller = authored_call.map(|(caller, _)| caller).or_else(|| {
        state.call.as_ref().and_then(|call| {
            matches!(
                call.phase,
                exchange_protocol::CallPhase::Completed
                    | exchange_protocol::CallPhase::Missed
                    | exchange_protocol::CallPhase::Misrouted
            )
            .then_some(call.caller_line)
        })
    });
    let mut next_calls = Vec::with_capacity(calls.len());
    let mut focused_call = None;
    let mut game_phase = state.game_phase.clone();
    let mut shift = state.shift.clone();
    let mut routing_receipt = None;
    let mut story_outcome = None;
    let mut story_event_complete = false;

    if calls.is_empty() {
        let single = advance_single_call(
            state,
            input,
            previous_crank_rotation_timestamps,
            authored_call,
            connected_callers_ready.is_none_or(|callers| {
                authored_call.is_some_and(|(caller, _)| callers.contains(&caller))
            }),
            auto_complete_connected,
        );
        if let Some(call) = single.call.clone() {
            focused_call = Some(call.clone());
            next_calls.push(call);
        }
        let line_lamps = lamps_for_calls(&next_calls);
        return CallTransition {
            calls: next_calls,
            call: focused_call,
            line_lamps,
            game_phase: single.game_phase,
            shift: single.shift,
            routing_receipt: single.routing_receipt,
            story_outcome: single.story_outcome,
            story_event_complete: single.story_event_complete,
        };
    }

    for call in calls {
        let mut single_state = state.clone();
        single_state.call = Some(call.clone());
        single_state.calls = vec![call.clone()];
        let mut single = advance_single_call(
            &single_state,
            input,
            previous_crank_rotation_timestamps,
            None,
            connected_callers_ready.is_none_or(|callers| callers.contains(&call.caller_line)),
            auto_complete_connected,
        );
        if previous_focused_line == Some(call.caller_line)
            && operator_line.is_some()
            && operator_line != Some(call.caller_line)
            && call.phase == exchange_protocol::CallPhase::OperatorSession
        {
            single.call = Some(exchange_protocol::CallStatus {
                phase: exchange_protocol::CallPhase::Held,
                ..call.clone()
            });
            single.line_lamps = lamps_for_call(single.call.as_ref());
        }
        if let Some(next_call) = single.call {
            if operator_line == Some(next_call.caller_line)
                || (operator_line.is_none() && previous_focused_line == Some(next_call.caller_line))
            {
                focused_call = Some(next_call.clone());
            }
            next_calls.push(next_call);
        }
        game_phase = single.game_phase;
        shift = single.shift;
        if authored_caller == Some(call.caller_line) {
            story_outcome = single.story_outcome;
            story_event_complete = single.story_event_complete;
            routing_receipt = single.routing_receipt;
        } else if previous_focused_line == Some(call.caller_line) {
            story_event_complete = single.story_event_complete;
        }
    }

    if focused_call.is_none() && previous_focused_line.is_some() {
        focused_call = next_calls
            .iter()
            .find(|call| Some(call.caller_line) == previous_focused_line)
            .cloned();
    }
    shift.active_call_count = next_calls
        .iter()
        .filter(|call| {
            !matches!(
                call.phase,
                exchange_protocol::CallPhase::Completed
                    | exchange_protocol::CallPhase::Missed
                    | exchange_protocol::CallPhase::Misrouted
                    | exchange_protocol::CallPhase::Failed
            )
        })
        .count()
        .min(u8::MAX as usize) as u8;
    CallTransition {
        calls: next_calls.clone(),
        call: focused_call,
        line_lamps: lamps_for_calls(&next_calls),
        game_phase,
        shift,
        routing_receipt,
        story_outcome,
        story_event_complete,
    }
}

fn advance_single_call(
    state: &StateOutput,
    input: &InputState,
    previous_crank_rotation_timestamps: [u64; 4],
    authored_call: Option<(u8, u8)>,
    connected_call_ready: bool,
    auto_complete_connected: bool,
) -> SingleCallTransition {
    if state.call.is_none() {
        let Some((caller_line, callee)) = authored_call else {
            if input.cord_topology.is_empty() {
                return SingleCallTransition {
                    line_lamps: [false; 12],
                    call: None,
                    game_phase: state.game_phase.clone(),
                    shift: state.shift.clone(),
                    routing_receipt: None,
                    story_outcome: None,
                    story_event_complete: false,
                };
            }
            return unchanged_transition(state);
        };
        let caller = PortId::Subscriber(caller_line);
        let operator_cord = has_exact_cords(&input.cord_topology, &[(&caller, &PortId::Operator)]);
        if !input.cord_topology.is_empty() && !operator_cord {
            return unchanged_transition(state);
        }
        let phase = if operator_cord {
            exchange_protocol::CallPhase::OperatorSession
        } else {
            exchange_protocol::CallPhase::Waiting
        };
        let call = exchange_protocol::CallStatus {
            caller_line,
            requested_callee_line: callee,
            phase,
        };
        let active_call_count = 1;
        return SingleCallTransition {
            line_lamps: lamps_for_call(Some(&call)),
            call: Some(call),
            game_phase: if operator_cord {
                GamePhase::Shift
            } else {
                state.game_phase.clone()
            },
            shift: ShiftStatus {
                active_call_count,
                phase: if operator_cord {
                    ShiftPhase::Active
                } else {
                    state.shift.phase.clone()
                },
                ..state.shift.clone()
            },
            routing_receipt: None,
            story_outcome: None,
            story_event_complete: false,
        };
    }

    let call = state.call.as_ref().expect("call checked above");
    let caller = PortId::Subscriber(call.caller_line);
    let callee = PortId::Subscriber(call.requested_callee_line);
    let caller_operator = has_exact_cords(&input.cord_topology, &[(&caller, &PortId::Operator)]);
    let ring_generator = has_ring_generator(&input.cord_topology, &caller, &callee);
    let direct_circuit = has_exact_cords(&input.cord_topology, &[(&caller, &callee)])
        || has_tap_bridge_circuit(&input.cord_topology, &caller, &callee);

    let mut next_call = call.clone();
    let mut next_game_phase = state.game_phase.clone();
    let mut next_shift = state.shift.clone();
    let mut routing_receipt = None;
    let mut story_outcome = None;
    let story_event_complete = false;
    let invalid_circuit = has_wrong_direct_circuit(&input.cord_topology, &caller, &callee);

    if call.phase == exchange_protocol::CallPhase::AwaitingRouting && input.cord_topology.is_empty()
    {
        next_call.phase = exchange_protocol::CallPhase::Missed;
        story_outcome = Some(StoryOutcome::Missed);
        return SingleCallTransition {
            line_lamps: lamps_for_call(Some(&next_call)),
            call: Some(next_call),
            game_phase: next_game_phase,
            shift: next_shift,
            routing_receipt,
            story_outcome,
            story_event_complete,
        };
    }

    match call.phase {
        exchange_protocol::CallPhase::Waiting => {
            if caller_operator {
                next_call.phase = exchange_protocol::CallPhase::OperatorSession;
                next_game_phase = GamePhase::Shift;
                next_shift.phase = ShiftPhase::Active;
            } else if !input.cord_topology.is_empty() {
                return unchanged_transition(state);
            }
        }
        exchange_protocol::CallPhase::OperatorSession => {
            if direct_circuit {
                next_call.phase = exchange_protocol::CallPhase::Connected;
                next_shift.completed_routings += 1;
                next_game_phase = GamePhase::Shift;
                story_outcome = Some(StoryOutcome::Success);
                routing_receipt = Some(format!(
                    "ROUTING {} -> {}",
                    call.caller_line, call.requested_callee_line
                ));
            } else if ring_generator {
                if !crank_satisfies_ringing(
                    input.crank_rotation_timestamps,
                    previous_crank_rotation_timestamps,
                ) {
                    return unchanged_transition(state);
                }
                next_call.phase = exchange_protocol::CallPhase::Ringing;
            } else if input.cord_topology.is_empty() {
                next_call.phase = exchange_protocol::CallPhase::AwaitingRouting;
            } else if !caller_operator {
                return unchanged_transition(state);
            }
        }
        exchange_protocol::CallPhase::AwaitingRouting => {
            if direct_circuit {
                next_call.phase = exchange_protocol::CallPhase::Connected;
                next_shift.completed_routings += 1;
                next_game_phase = GamePhase::Shift;
                story_outcome = Some(StoryOutcome::Success);
                routing_receipt = Some(format!(
                    "ROUTING {} -> {}",
                    call.caller_line, call.requested_callee_line
                ));
            } else if invalid_circuit {
                next_call.phase = exchange_protocol::CallPhase::Misrouted;
                story_outcome = Some(StoryOutcome::Invalid);
            } else if ring_generator {
                if !crank_satisfies_ringing(
                    input.crank_rotation_timestamps,
                    previous_crank_rotation_timestamps,
                ) {
                    return unchanged_transition(state);
                }
                next_call.phase = exchange_protocol::CallPhase::Ringing;
            } else if caller_operator {
                next_call.phase = exchange_protocol::CallPhase::OperatorSession;
            } else if !input.cord_topology.is_empty() {
                return unchanged_transition(state);
            }
        }
        exchange_protocol::CallPhase::Held => {
            if caller_operator {
                next_call.phase = exchange_protocol::CallPhase::OperatorSession;
                next_game_phase = GamePhase::Shift;
                next_shift.phase = ShiftPhase::Active;
            } else if !input.cord_topology.is_empty() {
                return unchanged_transition(state);
            }
        }
        exchange_protocol::CallPhase::Ringing => {
            if direct_circuit {
                next_call.phase = exchange_protocol::CallPhase::Connected;
                next_shift.completed_routings += 1;
                next_game_phase = GamePhase::Shift;
                story_outcome = Some(StoryOutcome::Success);
                routing_receipt = Some(format!(
                    "ROUTING {} -> {}",
                    call.caller_line, call.requested_callee_line
                ));
            } else if invalid_circuit {
                next_call.phase = exchange_protocol::CallPhase::Misrouted;
                story_outcome = Some(StoryOutcome::Invalid);
            } else if ring_generator {
                if crank_satisfies_ringing(
                    input.crank_rotation_timestamps,
                    previous_crank_rotation_timestamps,
                ) {
                    next_call.phase = exchange_protocol::CallPhase::Ringing;
                } else {
                    next_call.phase = exchange_protocol::CallPhase::AwaitingRouting;
                }
            } else if caller_operator {
                next_call.phase = exchange_protocol::CallPhase::OperatorSession;
            } else if input.cord_topology.is_empty() {
                next_call.phase = exchange_protocol::CallPhase::AwaitingRouting;
            } else {
                return unchanged_transition(state);
            }
        }
        exchange_protocol::CallPhase::Connected => {
            if auto_complete_connected && direct_circuit && connected_call_ready {
                next_shift.active_call_count = 0;
                return SingleCallTransition {
                    call: None,
                    line_lamps: [false; 12],
                    game_phase: next_game_phase,
                    shift: next_shift,
                    routing_receipt: None,
                    story_outcome: None,
                    story_event_complete: true,
                };
            } else if direct_circuit || !connected_call_ready {
                return unchanged_transition(state);
            } else if input.cord_topology.is_empty() {
                next_shift.active_call_count = 0;
                return SingleCallTransition {
                    call: None,
                    line_lamps: [false; 12],
                    game_phase: next_game_phase,
                    shift: next_shift,
                    routing_receipt: None,
                    story_outcome: None,
                    story_event_complete: true,
                };
            } else {
                return unchanged_transition(state);
            }
        }
        exchange_protocol::CallPhase::Completed => {
            if input.cord_topology.is_empty() {
                return SingleCallTransition {
                    call: None,
                    line_lamps: [false; 12],
                    game_phase: next_game_phase,
                    shift: next_shift,
                    routing_receipt: None,
                    story_outcome: None,
                    story_event_complete: true,
                };
            }
            if !direct_circuit {
                return unchanged_transition(state);
            }
        }
        exchange_protocol::CallPhase::Missed
        | exchange_protocol::CallPhase::Misrouted
        | exchange_protocol::CallPhase::Failed => {
            if input.cord_topology.is_empty() {
                next_shift.active_call_count = 0;
                return SingleCallTransition {
                    call: None,
                    line_lamps: [false; 12],
                    game_phase: next_game_phase,
                    shift: next_shift,
                    routing_receipt: None,
                    story_outcome: None,
                    story_event_complete: true,
                };
            }
        }
    }

    next_shift.active_call_count =
        u8::from(next_call.phase != exchange_protocol::CallPhase::Completed);
    SingleCallTransition {
        line_lamps: lamps_for_call(Some(&next_call)),
        call: Some(next_call),
        game_phase: next_game_phase,
        shift: next_shift,
        routing_receipt,
        story_outcome,
        story_event_complete,
    }
}

fn unchanged_transition(state: &StateOutput) -> SingleCallTransition {
    SingleCallTransition {
        call: state.call.clone(),
        line_lamps: state.line_lamps,
        game_phase: state.game_phase.clone(),
        shift: state.shift.clone(),
        routing_receipt: None,
        story_outcome: None,
        story_event_complete: false,
    }
}

fn unchanged_call_transition(state: &StateOutput) -> CallTransition {
    let calls = if state.calls.is_empty() {
        state.call.clone().into_iter().collect()
    } else {
        state.calls.clone()
    };
    CallTransition {
        calls,
        call: state.call.clone(),
        line_lamps: state.line_lamps,
        game_phase: state.game_phase.clone(),
        shift: state.shift.clone(),
        routing_receipt: None,
        story_outcome: None,
        story_event_complete: false,
    }
}

fn direct_routing_topology(input: &InputState, call: &exchange_protocol::CallStatus) -> bool {
    let caller = PortId::Subscriber(call.caller_line);
    let callee = PortId::Subscriber(call.requested_callee_line);
    has_exact_cords(&input.cord_topology, &[(&caller, &callee)])
        || has_tap_bridge_circuit(&input.cord_topology, &caller, &callee)
}

fn has_direct_subscriber_circuit(input: &InputState, caller_line: u8) -> bool {
    let caller = PortId::Subscriber(caller_line);
    input.cord_topology.len() == 1
        && input.cord_topology.iter().any(|cord| {
            (cord.first == caller && matches!(cord.second, PortId::Subscriber(_)))
                || (cord.second == caller && matches!(cord.first, PortId::Subscriber(_)))
        })
}

fn crank_satisfies_ringing(current: [u64; 4], previous: [u64; 4]) -> bool {
    current[3] > previous[3]
}

fn valid_crank_history(timestamps: [u64; 4]) -> bool {
    let mut previous = 0;
    let mut nonzero_seen = false;
    for timestamp in timestamps {
        if timestamp == 0 {
            if nonzero_seen {
                return false;
            }
        } else {
            if timestamp <= previous {
                return false;
            }
            previous = timestamp;
            nonzero_seen = true;
        }
    }
    true
}

fn tuning_reduces_interference(tuning: &TuningState) -> bool {
    (384..=640).contains(&tuning.coarse) && (384..=640).contains(&tuning.fine)
}

fn has_diegetic_interference(story_node_id: &str) -> bool {
    story_node_id == "shift_2_call" || story_node_id == "hardware_demo_interference_call"
}

fn interference_level(story_node_id: &str, tuning: &TuningState) -> u8 {
    if !has_diegetic_interference(story_node_id) {
        return 0;
    }
    if tuning_reduces_interference(tuning) {
        return 0;
    }
    let distance =
        u32::from(tuning.coarse.abs_diff(512)).saturating_add(u32::from(tuning.fine.abs_diff(512)));
    (distance.saturating_mul(100) / 1024).clamp(10, 100) as u8
}

fn call_patience_seconds(shift: u8) -> u32 {
    match shift {
        1 => 90,
        2 => 120,
        3 => 150,
        _ => 90,
    }
}

fn accelerated_clock_seconds(real_elapsed_seconds: u64) -> u32 {
    DEMO_SHIFT_START_SECONDS
        .saturating_add(
            real_elapsed_seconds
                .min(DEMO_SHIFT_DURATION_SECONDS)
                .saturating_mul(60),
        )
        .min(u32::MAX as u64) as u32
}

fn speaker_is_active(input: &InputState, state: &StateOutput) -> bool {
    let held = &input.held_controls;
    let operator_active = held.ptt
        && input
            .cord_topology
            .iter()
            .any(|cord| cord.first == PortId::Operator || cord.second == PortId::Operator);
    let tap_active = tap_bridge_monitoring(input, state).is_some();
    operator_active || held.police || held.ems || tap_active
}

fn service_from_controls(held: &exchange_protocol::HeldControls) -> Option<ServiceKind> {
    if held.police {
        Some(ServiceKind::Police)
    } else if held.ems {
        Some(ServiceKind::Ems)
    } else {
        None
    }
}

fn service_for_action(action: OperatorTextAction) -> Option<ServiceKind> {
    match action {
        OperatorTextAction::CallEms => Some(ServiceKind::Ems),
        OperatorTextAction::ReportPolice => Some(ServiceKind::Police),
        _ => None,
    }
}

fn apply_service_transition(
    backend: &mut Backend,
    state: &mut StateOutput,
    input: &InputState,
    settling: bool,
) {
    let held_service = service_from_controls(&input.held_controls);
    match state
        .service_call
        .as_ref()
        .map(|call| (call.service, call.phase))
    {
        Some((service, ServiceCallPhase::Active)) => {
            if held_service.is_none() {
                state.service_call = Some(ServiceCallStatus {
                    service,
                    phase: ServiceCallPhase::Completed,
                });
                state.shift.completed_service_calls += 1;
            }
        }
        _ if held_service.is_some() && backend.last_held_service != held_service => {
            let service = held_service.expect("service checked above");
            let required_service = backend.required_service_kind_for_shift(state.shift.number);
            if let Some(required_service) = required_service
                && service != required_service
            {
                state.debug.messages.push(BackendDiagnostic {
                    code: if required_service == ServiceKind::Police {
                        "police_service_required".to_string()
                    } else {
                        "required_service_kind".to_string()
                    },
                    message: format!(
                        "this Shift requires the {} Service Call",
                        service_label(required_service)
                    ),
                });
                if state.debug.messages.len() > MAX_FAULTS {
                    state.debug.messages.remove(0);
                }
            } else if required_service == Some(ServiceKind::Police)
                && backend.directory_lookup_id != Some(2)
            {
                state.debug.messages.push(BackendDiagnostic {
                    code: "directory_report_required".to_string(),
                    message: "look up Directory 0002 before placing the Police Service Call"
                        .to_string(),
                });
                if state.debug.messages.len() > MAX_FAULTS {
                    state.debug.messages.remove(0);
                }
            } else {
                state.service_call = Some(ServiceCallStatus {
                    service,
                    phase: ServiceCallPhase::Active,
                });
                let served_line = state.call.as_ref().map(|call| call.caller_line);
                if let Some(served_line) = served_line {
                    state.calls.retain(|call| call.caller_line != served_line);
                }
                state.call = None;
                state.line_lamps = lamps_for_calls(&state.calls);
            }
        }
        _ => {}
    }

    if settling
        && !backend.service_error_recorded
        && state.shift.completed_service_calls < state.shift.required_service_calls
    {
        record_service_error(state, ServiceErrorKind::MissedRequiredServiceCall);
        backend.service_error_recorded = true;
    }
}

fn service_label(service: ServiceKind) -> &'static str {
    match service {
        ServiceKind::Police => "POLICE",
        ServiceKind::Ems => "EMS",
    }
}

fn record_service_error(state: &mut StateOutput, kind: ServiceErrorKind) {
    state.shift.service_errors += 1;
    if let Some(error) = state
        .shift
        .service_error_counts
        .iter_mut()
        .find(|error| error.kind == kind)
    {
        error.count += 1;
    } else {
        state
            .shift
            .service_error_counts
            .push(ServiceErrorCount { kind, count: 1 });
    }
}

fn append_printer(state: &mut StateOutput, text: &str) {
    let entry_id = state
        .printer_output
        .last()
        .map_or(1, |entry| entry.entry_id + 1);
    state.printer_output.push(PrinterEntry {
        entry_id,
        text: text.to_string(),
    });
}

fn ending_receipt(conclusion: &str) -> String {
    format!("ENDING // {conclusion}")
}

fn directory_id(digits: [u8; 4]) -> u16 {
    digits
        .iter()
        .fold(0_u16, |value, digit| value * 10 + *digit as u16)
}

fn known_directory_id(id: u16) -> bool {
    matches!(id, 1..=5)
}

fn has_wrong_direct_circuit(cords: &[CordConnection], caller: &PortId, callee: &PortId) -> bool {
    cords.len() == 1
        && cords.iter().any(|cord| {
            let other = if &cord.first == caller {
                Some(&cord.second)
            } else if &cord.second == caller {
                Some(&cord.first)
            } else {
                None
            };
            matches!(other, Some(PortId::Subscriber(line)) if callee != &PortId::Subscriber(*line))
        })
}

fn lamps_for_call(call: Option<&exchange_protocol::CallStatus>) -> [bool; 12] {
    let mut lamps = [false; 12];
    let Some(call) = call else {
        return lamps;
    };
    if !call_lamp_active(&call.phase) {
        return lamps;
    }
    lamps[call.caller_line as usize] = true;
    if matches!(
        call.phase,
        exchange_protocol::CallPhase::Ringing
            | exchange_protocol::CallPhase::Connected
            | exchange_protocol::CallPhase::Completed
    ) {
        lamps[call.requested_callee_line as usize] = true;
    }
    lamps
}

fn lamps_for_calls(calls: &[exchange_protocol::CallStatus]) -> [bool; 12] {
    let mut lamps = [false; 12];
    for call in calls {
        if !call_lamp_active(&call.phase) {
            continue;
        }
        lamps[call.caller_line as usize] = true;
        if matches!(
            call.phase,
            exchange_protocol::CallPhase::Ringing
                | exchange_protocol::CallPhase::Connected
                | exchange_protocol::CallPhase::Completed
        ) {
            lamps[call.requested_callee_line as usize] = true;
        }
    }
    lamps
}

fn call_lamp_active(phase: &exchange_protocol::CallPhase) -> bool {
    !matches!(
        phase,
        exchange_protocol::CallPhase::Completed
            | exchange_protocol::CallPhase::Missed
            | exchange_protocol::CallPhase::Misrouted
            | exchange_protocol::CallPhase::Failed
    )
}

fn light_ring_generator_lines(
    lamps: &mut [bool; 12],
    cords: &[CordConnection],
    crank_is_recent: bool,
) {
    if !crank_is_recent {
        return;
    }

    for cord in cords {
        let line = match (&cord.first, &cord.second) {
            (PortId::Subscriber(line), PortId::RingGenerator)
            | (PortId::RingGenerator, PortId::Subscriber(line)) => Some(*line),
            _ => None,
        };
        if let Some(line) = line.filter(|line| *line < 12) {
            lamps[line as usize] = true;
        }
    }
}

fn has_tap_bridge_circuit(cords: &[CordConnection], caller: &PortId, callee: &PortId) -> bool {
    has_tap_bridge_circuit_on_bridge(cords, caller, callee, 1)
}

fn has_tap_bridge_circuit_on_bridge(
    cords: &[CordConnection],
    caller: &PortId,
    callee: &PortId,
    bridge: u8,
) -> bool {
    let first = PortId::Tap(bridge * 2 - 1);
    let second = PortId::Tap(bridge * 2);
    has_exact_cords(cords, &[(caller, &first), (callee, &second)])
        || has_exact_cords(cords, &[(caller, &second), (callee, &first)])
}

fn tap_bridge_monitoring(input: &InputState, state: &StateOutput) -> Option<u8> {
    let held = input.held_controls.tap;
    held.then(|| 1).filter(|bridge| {
        state.calls.iter().any(|call| {
            matches!(
                call.phase,
                exchange_protocol::CallPhase::Connected | exchange_protocol::CallPhase::Completed
            ) && has_tap_bridge_circuit_on_bridge(
                &input.cord_topology,
                &PortId::Subscriber(call.caller_line),
                &PortId::Subscriber(call.requested_callee_line),
                *bridge,
            )
        })
    })
}

fn has_ring_generator(cords: &[CordConnection], caller: &PortId, callee: &PortId) -> bool {
    has_exact_cords(
        cords,
        &[
            (caller, &PortId::Operator),
            (callee, &PortId::RingGenerator),
        ],
    )
}

fn has_exact_cords(cords: &[CordConnection], expected: &[(&PortId, &PortId)]) -> bool {
    cords.len() == expected.len()
        && expected.iter().all(|(first, second)| {
            cords.iter().any(|cord| {
                (&cord.first == *first && &cord.second == *second)
                    || (&cord.first == *second && &cord.second == *first)
            })
        })
}

pub fn serve(listener: TcpListener) -> io::Result<()> {
    serve_with_voice_and_debug(listener, None, None)
}

pub fn serve_with_voice(listener: TcpListener, voice_socket: Option<UdpSocket>) -> io::Result<()> {
    serve_with_voice_and_debug(listener, voice_socket, None)
}

pub fn serve_with_voice_and_debug(
    listener: TcpListener,
    voice_socket: Option<UdpSocket>,
    debug_listener: Option<TcpListener>,
) -> io::Result<()> {
    let backend = Arc::new(Mutex::new(Backend::new_north_neeladesh()));
    let voice_socket = voice_socket.map(Arc::new);
    if let Some(debug_listener) = debug_listener {
        let backend = Arc::clone(&backend);
        let diagnostics_backend = Arc::clone(&backend);
        thread::spawn(move || {
            if let Err(error) = serve_debug(debug_listener, backend)
                && let Ok(mut backend) = diagnostics_backend.lock()
            {
                backend.add_diagnostic(BackendDiagnostic {
                    code: "debug_surface_failed".to_string(),
                    message: error.to_string(),
                });
            }
        });
    }
    if let Some(voice_socket) = &voice_socket {
        let backend = Arc::clone(&backend);
        let socket = voice_socket
            .try_clone()
            .map_err(|error| io::Error::other(format!("voice socket clone failed: {error}")))?;
        thread::spawn(move || serve_voice(socket, backend));
    }
    for connection in listener.incoming() {
        let stream = connection?;
        let backend = Arc::clone(&backend);
        let voice_socket = voice_socket.as_ref().map(Arc::clone);
        let diagnostics_backend = Arc::clone(&backend);
        thread::spawn(move || {
            if let Err(error) = handle_connection_with_voice(stream, backend, voice_socket)
                && let Ok(mut backend) = diagnostics_backend.lock()
            {
                backend.add_diagnostic(BackendDiagnostic {
                    code: "frontend_connection_failed".to_string(),
                    message: error.to_string(),
                });
            }
        });
    }
    Ok(())
}

pub fn serve_debug(listener: TcpListener, backend: Arc<Mutex<Backend>>) -> io::Result<()> {
    for connection in listener.incoming() {
        let stream = connection?;
        let backend = Arc::clone(&backend);
        let diagnostics_backend = Arc::clone(&backend);
        thread::spawn(move || {
            if let Err(error) = handle_debug_connection(stream, backend)
                && let Ok(mut backend) = diagnostics_backend.lock()
            {
                backend.add_diagnostic(BackendDiagnostic {
                    code: "debug_connection_failed".to_string(),
                    message: error.to_string(),
                });
            }
        });
    }
    Ok(())
}

fn handle_debug_connection(mut stream: TcpStream, backend: Arc<Mutex<Backend>>) -> io::Result<()> {
    loop {
        let request: DebugRequest = match read_frame(&mut stream) {
            Ok(request) => request,
            Err(exchange_protocol::FrameError::Io(error))
                if matches!(
                    error.kind(),
                    ErrorKind::UnexpectedEof | ErrorKind::ConnectionReset
                ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(io::Error::new(ErrorKind::InvalidData, error)),
        };
        let response = {
            let mut backend = backend
                .lock()
                .map_err(|_| io::Error::other("backend state lock poisoned"))?;
            if request.protocol_version != DEBUG_PROTOCOL_VERSION {
                DebugResponse {
                    protocol_version: DEBUG_PROTOCOL_VERSION,
                    accepted: false,
                    error: Some(protocol_error(
                        "unsupported_protocol_version",
                        format!("expected debug protocol version {DEBUG_PROTOCOL_VERSION}"),
                    )),
                    snapshot: backend.debug_snapshot(),
                    audio: None,
                }
            } else {
                backend.apply_debug_command(request.command)
            }
        };
        write_frame(&mut stream, &response)
            .map_err(|error| io::Error::new(ErrorKind::BrokenPipe, error))?;
    }
}

pub fn serve_voice(socket: UdpSocket, backend: Arc<Mutex<Backend>>) -> io::Result<()> {
    socket.set_read_timeout(Some(Duration::from_millis(50)))?;
    let mut datagram = [0_u8; 65_535];
    let mut next_audio_send = None;
    loop {
        let mut peer = None;
        let work = match socket.recv_from(&mut datagram) {
            Ok((length, received_peer)) => {
                peer = Some(received_peer);
                let mut backend = backend
                    .lock()
                    .map_err(|_| io::Error::other("backend state lock poisoned"))?;
                backend.apply_voice_datagram_from(&datagram[..length], Some(received_peer));
                backend.take_voice_input()
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                None
            }
            Err(error) => return Err(error),
        };
        let (outgoing, peer, control) = {
            let mut backend = backend
                .lock()
                .map_err(|_| io::Error::other("backend state lock poisoned"))?;
            (
                backend.take_voice_datagrams(),
                peer.or(backend.voice_peer),
                backend.take_voice_control(),
            )
        };
        if let Some(peer) = peer {
            send_voice_datagrams(&socket, peer, outgoing, &mut next_audio_send)?;
        }
        if let Some((control, peer)) = control {
            let datagram = encode_voice_control(&control).map_err(|error| {
                io::Error::other(format!("voice control encode failed: {error}"))
            })?;
            socket.send_to(&datagram, peer)?;
        }
        if let Some((
            input,
            voice_id,
            subscriber_line,
            callee_line,
            operator_knowledge,
            generation,
            conversation_id,
        )) = work
        {
            let backend = Arc::clone(&backend);
            thread::spawn(move || {
                run_voice_worker(
                    input,
                    voice_id,
                    subscriber_line,
                    callee_line,
                    operator_knowledge,
                    generation,
                    conversation_id,
                    backend,
                )
            });
        }
    }
}

fn send_voice_datagrams(
    socket: &UdpSocket,
    peer: SocketAddr,
    outgoing: Vec<Vec<u8>>,
    next_audio_send: &mut Option<Instant>,
) -> io::Result<()> {
    for datagram in outgoing {
        let audio_samples = RtpL16Packet::decode(&datagram)
            .ok()
            .map(|packet| packet.samples.len());
        if let Some(audio_samples) = audio_samples {
            if let Some(deadline) = *next_audio_send {
                let now = Instant::now();
                if deadline > now {
                    thread::sleep(deadline.duration_since(now));
                }
            }
            socket.send_to(&datagram, peer)?;
            *next_audio_send = Some(
                Instant::now()
                    + Duration::from_secs_f64(
                        audio_samples as f64 / VOICE_AUDIO_SAMPLE_RATE as f64,
                    ),
            );
        } else {
            socket.send_to(&datagram, peer)?;
        }
    }
    Ok(())
}

pub fn handle_connection(stream: TcpStream, backend: Arc<Mutex<Backend>>) -> io::Result<()> {
    handle_connection_with_voice(stream, backend, None)
}

fn handle_connection_with_voice(
    mut stream: TcpStream,
    backend: Arc<Mutex<Backend>>,
    voice_socket: Option<Arc<UdpSocket>>,
) -> io::Result<()> {
    loop {
        let message = match read_frame(&mut stream) {
            Ok(message) => message,
            Err(exchange_protocol::FrameError::Io(error))
                if matches!(
                    error.kind(),
                    ErrorKind::UnexpectedEof | ErrorKind::ConnectionReset
                ) =>
            {
                if let Ok(mut backend) = backend.lock() {
                    backend.mark_frontend_disconnected();
                }
                return Ok(());
            }
            Err(error) => return Err(io::Error::new(ErrorKind::InvalidData, error)),
        };
        let (response, voice_control) = {
            let mut backend = backend
                .lock()
                .map_err(|_| io::Error::other("backend state lock poisoned"))?;
            let response = backend.apply_input_message(message);
            (response, backend.take_voice_control())
        };
        if let (Some(socket), Some((control, peer))) = (voice_socket.as_ref(), voice_control) {
            let datagram = encode_voice_control(&control).map_err(|error| {
                io::Error::other(format!("voice control encode failed: {error}"))
            })?;
            socket.send_to(&datagram, peer)?;
        }
        write_frame(&mut stream, &response)
            .map_err(|error| io::Error::new(ErrorKind::BrokenPipe, error))?;
    }
}

fn validate_message(backend: &Backend, message: &InputMessage) -> Result<(), ProtocolError> {
    if message.protocol_version != PROTOCOL_VERSION {
        return Err(protocol_error(
            "unsupported_protocol_version",
            format!("expected protocol version {PROTOCOL_VERSION}"),
        ));
    }
    if message.input_sequence == 0 {
        return Err(protocol_error(
            "invalid_input_sequence",
            "input_sequence must be positive",
        ));
    }
    if message.input_sequence
        <= backend
            .last_request
            .as_ref()
            .map_or(0, |request| request.input_sequence)
    {
        return Err(protocol_error(
            "duplicate_input_sequence",
            "input_sequence must increase, or repeat the exact previous request",
        ));
    }
    if message.expected_state_revision != backend.state_revision {
        return Err(protocol_error(
            "stale_state_revision",
            format!(
                "expected state revision {}, received {}",
                backend.state_revision, message.expected_state_revision
            ),
        ));
    }

    let input = &message.input;
    validate_cords(&input.cord_topology)?;
    if input.directory_digits.iter().any(|digit| *digit > 9) {
        return Err(protocol_error(
            "invalid_directory_digits",
            "directory digits must be decimal digits",
        ));
    }
    if !valid_crank_history(input.crank_rotation_timestamps) {
        return Err(protocol_error(
            "invalid_crank_timestamps",
            "crank rotation timestamps must be strictly increasing after leading zeroes",
        ));
    }
    if input.tuning.coarse > 1_023 || input.tuning.fine > 1_023 {
        return Err(protocol_error(
            "invalid_tuning",
            "tuning values must be between 0 and 1023",
        ));
    }
    if input.debug.device_faults.len() > MAX_FAULTS
        || input
            .debug
            .device_faults
            .iter()
            .any(|fault| fault.len() > 128)
    {
        return Err(protocol_error(
            "invalid_diagnostics",
            "frontend diagnostics contain too many or too-long faults",
        ));
    }
    Ok(())
}

fn validate_cords(cords: &[CordConnection]) -> Result<(), ProtocolError> {
    if cords.len() > MAX_CORDS {
        return Err(protocol_error(
            "too_many_cords",
            "an input cannot contain more than 8 cords",
        ));
    }
    let mut ports = Vec::with_capacity(cords.len() * 2);
    for cord in cords {
        if cord.first == cord.second {
            return Err(protocol_error(
                "invalid_cord",
                "a cord must connect two different ports",
            ));
        }
        validate_port(&cord.first)?;
        validate_port(&cord.second)?;
        if ports.contains(&cord.first) || ports.contains(&cord.second) {
            return Err(protocol_error(
                "duplicate_port",
                "a port may appear in only one cord",
            ));
        }
        ports.push(cord.first.clone());
        ports.push(cord.second.clone());
    }
    Ok(())
}

fn validate_port(port: &PortId) -> Result<(), ProtocolError> {
    match port {
        PortId::Subscriber(line) if *line < 12 => Ok(()),
        PortId::Tap(index) if (1..=2).contains(index) => Ok(()),
        PortId::Operator | PortId::RingGenerator => Ok(()),
        PortId::Subscriber(_) => Err(protocol_error(
            "invalid_port",
            "subscriber ports must be numbered 0 through 11",
        )),
        PortId::Tap(_) => Err(protocol_error(
            "invalid_port",
            "Tap Bridge jacks must be tap_1 or tap_2",
        )),
    }
}

fn rejected_response(
    input_sequence: u64,
    error: ProtocolError,
    state_revision: u64,
    output: &StateOutput,
) -> StateMessage {
    StateMessage {
        protocol_version: PROTOCOL_VERSION,
        input_sequence,
        accepted: false,
        error: Some(error),
        state_revision,
        output: output.clone(),
    }
}

fn protocol_error(code: &str, message: impl Into<String>) -> ProtocolError {
    ProtocolError {
        code: code.to_string(),
        message: message.into(),
    }
}

struct ReceivedCapture {
    samples: Option<Vec<i16>>,
    active: bool,
}

impl ReceivedCapture {
    fn new(samples: Vec<i16>) -> Self {
        Self {
            samples: Some(samples),
            active: false,
        }
    }
}

impl MicrophoneCapture for ReceivedCapture {
    fn start(&mut self) -> Result<(), VoiceError> {
        self.active = true;
        Ok(())
    }

    fn finish(&mut self) -> Result<Vec<i16>, VoiceError> {
        if !self.active {
            return Err(VoiceError::new(
                "capture_not_started",
                "remote voice capture was not started",
            ));
        }
        self.active = false;
        Ok(self.samples.take().unwrap_or_default())
    }
}

struct BackendVoiceOutput {
    backend: Arc<Mutex<Backend>>,
    generation: u64,
    conversation_id: u64,
}

impl VoiceOutput for BackendVoiceOutput {
    fn status(&mut self, message: exchange_protocol::VoiceStatusMessage) -> Result<(), VoiceError> {
        let relay_message = exchange_protocol::VoiceStatusMessage {
            transcript: None,
            response_text: None,
            ..message.clone()
        };
        let datagram = exchange_protocol::encode_voice_status(&relay_message)
            .map_err(|error| VoiceError::new("voice_status_encode_failed", error.to_string()))?;
        let mut backend = self.backend.lock().map_err(|_| {
            VoiceError::new("voice_backend_lock_failed", "backend state lock poisoned")
        })?;
        if backend.run_generation != self.generation {
            return Err(VoiceError::new(
                "stale_voice_worker",
                "voice worker belongs to a reset Run",
            ));
        }
        backend.voice_status = Some(message.status);
        if message.transcript.is_some() {
            backend.voice_transcript = message.transcript.clone();
        }
        if message.response_text.is_some() {
            backend.voice_response_text = message.response_text.clone();
        }
        backend.voice_speaker_active = matches!(message.status, VoiceStatus::Playing);
        backend.update_voice_conversation(
            self.conversation_id,
            message.status,
            message.error.clone(),
            message.transcript.clone(),
            message.response_text.clone(),
        );
        if message.status == VoiceStatus::Failed {
            backend.add_diagnostic(BackendDiagnostic {
                code: "voice_worker_failed".to_string(),
                message: message.error.map_or_else(
                    || "voice worker failed without details".to_string(),
                    |error| format!("{}: {}", error.code, error.message),
                ),
            });
        }
        backend.queue_voice_datagram(datagram);
        Ok(())
    }

    fn audio(&mut self, packet: exchange_protocol::RtpL16Packet) -> Result<(), VoiceError> {
        let mut backend = self.backend.lock().map_err(|_| {
            VoiceError::new("voice_backend_lock_failed", "backend state lock poisoned")
        })?;
        if backend.run_generation != self.generation {
            return Err(VoiceError::new(
                "stale_voice_worker",
                "voice worker belongs to a reset Run",
            ));
        }
        if let Some(conversation) = backend.voice_conversation_mut(self.conversation_id) {
            conversation.tts_audio.extend_from_slice(&packet.samples);
            conversation.summary.tts_samples =
                conversation.tts_audio.len().min(u32::MAX as usize) as u32;
        }
        backend.queue_voice_datagram(packet.encode());
        Ok(())
    }
}

fn run_voice_worker(
    input: VoiceInputAudioMessage,
    voice_id: String,
    subscriber_line: u8,
    callee_line: Option<u8>,
    operator_knowledge: Vec<String>,
    generation: u64,
    conversation_id: u64,
    backend: Arc<Mutex<Backend>>,
) {
    let session_id = input.session_id;
    let turn_id = input.turn_id;
    let state_revision = input.state_revision;
    let output: Box<dyn VoiceOutput> = Box::new(BackendVoiceOutput {
        backend: Arc::clone(&backend),
        generation,
        conversation_id,
    });
    let result = run_voice_worker_session(
        input,
        voice_id,
        subscriber_line,
        callee_line,
        operator_knowledge,
        output,
    );
    if let Err(error) = result {
        let mut backend = backend
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if backend.run_generation == generation {
            if let Ok(datagram) =
                exchange_protocol::encode_voice_status(&exchange_protocol::VoiceStatusMessage {
                    protocol_version: exchange_protocol::VOICE_PROTOCOL_VERSION,
                    session_id,
                    turn_id,
                    state_revision,
                    status: VoiceStatus::Failed,
                    transcript: None,
                    response_text: None,
                    error: Some(ProtocolError {
                        code: error.code.clone(),
                        message: error.message.clone(),
                    }),
                })
            {
                backend.queue_voice_datagram(datagram);
            }
            backend.voice_status = Some(VoiceStatus::Failed);
            backend.voice_speaker_active = false;
            backend.fail_voice_conversation(conversation_id, &error);
            backend.add_diagnostic(BackendDiagnostic {
                code: "voice_worker_failed".to_string(),
                message: format!("{}: {}", error.code, error.message),
            });
            backend.voice_worker_active = false;
        }
    } else {
        let mut backend = backend
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if backend.run_generation == generation {
            backend.finish_voice_worker();
        }
    }
}

fn run_voice_worker_session(
    input: VoiceInputAudioMessage,
    voice_id: String,
    subscriber_line: u8,
    callee_line: Option<u8>,
    operator_knowledge: Vec<String>,
    output: Box<dyn VoiceOutput>,
) -> Result<(), VoiceError> {
    let stt = CommandSpec::from_words(&env::var("NN_VOICE_STT_COMMAND").map_err(|_| {
        VoiceError::new(
            "voice_worker_not_configured",
            "NN_VOICE_STT_COMMAND is not configured",
        )
    })?)?;
    let dialogue =
        CommandSpec::from_words(&env::var("NN_VOICE_DIALOGUE_COMMAND").map_err(|_| {
            VoiceError::new(
                "voice_worker_not_configured",
                "NN_VOICE_DIALOGUE_COMMAND is not configured",
            )
        })?)?;
    let tts = CommandSpec::from_words(&env::var("NN_VOICE_TTS_COMMAND").map_err(|_| {
        VoiceError::new(
            "voice_worker_not_configured",
            "NN_VOICE_TTS_COMMAND is not configured",
        )
    })?)?;
    let tts: Box<dyn TextToSpeech> = if env::var_os("NN_VOICE_TTS_PERSISTENT").is_some() {
        Box::new(PersistentQwen3TtsCommand::new(tts)?)
    } else {
        Box::new(Qwen3TtsCommand::new(tts))
    };
    let mut session = OperatorSession::new(
        input.session_id,
        input.state_revision,
        north_response_context(subscriber_line, callee_line, voice_id, operator_knowledge),
        Box::new(ReceivedCapture::new(input.samples)),
        Box::new(CommandSpeechToText::new(stt)),
        Box::new(CommandDialogueGenerator::new(dialogue)),
        tts,
        output,
    )?;
    session.start_ptt()?;
    session.release_ptt().map(|_| ())
}

fn north_response_context(
    subscriber_line: u8,
    callee_line: Option<u8>,
    voice_id: String,
    operator_knowledge: Vec<String>,
) -> ResponseContext {
    let (subscriber_id, name, personality, goal, relationship) = match subscriber_line {
        0 => (
            1,
            "Anika Roy",
            "quiet hospital nurse",
            "Speak with the person at the requested destination",
            "the person I am calling",
        ),
        1 | 4 => (
            2,
            "Nayan Boro",
            "careful government secretary",
            "Get the official bulletin aired",
            "the Republic Secretariat",
        ),
        2 => (
            3,
            "Inspector Rakesh Nahal",
            "controlled ministry investigator",
            "Verify the listed residents",
            "Home Affairs",
        ),
        3 => (
            6,
            "Captain Varo",
            "disciplined army officer",
            "Keep the cantonment line controlled",
            "the Army command",
        ),
        5 => (
            12,
            "Meera Tal",
            "calm embassy contact under pressure",
            "Reach the embassy contact",
            "the South Neeladesh Embassy",
        ),
        6 => (
            7,
            "Tomas Vale",
            "nervous hotel desk clerk",
            "Reach the requested contact",
            "the Hotel desk",
        ),
        7 => (
            5,
            "Javed Rahman",
            "methodical mining office clerk",
            "Report what the office knows",
            "the Mining Office",
        ),
        8 => (
            8,
            "Paro Sen",
            "careful colony resident",
            "Reach the correct household",
            "Ratan Colony",
        ),
        9 => (
            14,
            "Rafi Alam",
            "worried Shapla Apartments resident",
            "Get help to my mother",
            "Asha Sen",
        ),
        10 => (
            8,
            "Bikram Sen",
            "alert market clerk",
            "Reach the requested contact",
            "the Market",
        ),
        11 => (
            13,
            "Mira Halek",
            "observant station worker",
            "Report what happened at the station",
            "Central Station",
        ),
        _ => (
            0,
            "Unknown Caller",
            "guarded local caller",
            "Reach the requested place",
            "the exchange",
        ),
    };
    let requested_place = callee_line
        .map(north_place_for_line)
        .unwrap_or("the requested place");
    let mut permitted_knowledge = vec![KnowledgeRecord {
        fact: format!(
            "{name} is calling from {}",
            north_place_for_line(subscriber_line)
        ),
        learned_from: "authored_call_premise".to_string(),
    }];
    permitted_knowledge.extend(operator_knowledge.into_iter().map(|fact| KnowledgeRecord {
        fact,
        learned_from: "tap_bridge_monitoring".to_string(),
    }));
    ResponseContext {
        profile: SubscriberProfile {
            subscriber_id,
            name: name.to_string(),
            voice_id,
            personality: personality.to_string(),
            baseline_goals: vec![goal.to_string()],
            initial_perspective: "The exchange is under observation".to_string(),
            relationships: vec![RelationshipNote {
                subject: relationship.to_string(),
                note: "Relevant to the current Call".to_string(),
            }],
            permitted_actions: vec!["request_routing".to_string()],
        },
        caller_place: north_place_for_line(subscriber_line).to_string(),
        requested_place: requested_place.to_string(),
        known_places: (0..12).map(north_place_for_line).map(str::to_string).collect(),
        subscriber_goal: goal.to_string(),
        call_premise: format!(
            "Request a connection to {requested_place}. Do not volunteer the destination unless asked."
        ),
        story_beat_direction: "Answer ordinary questions naturally. If asked for the destination, name the place exactly. Add harmless everyday detail without changing the call.".to_string(),
        permitted_knowledge,
        beliefs: Vec::new(),
        relationship_notes: Vec::new(),
        memories: Vec::new(),
        recent_conversation: Vec::new(),
        current_input: None,
    }
}

fn north_place_for_line(line: u8) -> &'static str {
    match line {
        0 => "Neeladesh Central Hospital",
        1 => "Republic Secretariat",
        2 => "Home Affairs Annex",
        3 => "Cantonment",
        4 => "National Radio Building",
        5 => "South Neeladesh Embassy",
        6 => "Hotel Meridian",
        7 => "Mining Office",
        8 => "Ratan Colony",
        9 => "Shapla Apartments",
        10 => "Old Market",
        11 => "Central Station",
        _ => "the exchange",
    }
}

fn simple_place_for_line(line: u8) -> &'static str {
    match line {
        0 => "RAIL DISPATCH",
        1 => "KHARAD CLINIC",
        2 => "RATION OFFICE",
        3 => "BORDER DEPOT",
        4 => "FOUNDRY APTS",
        5 => "BORDER POST",
        6 => "LABOUR OFFICE",
        7 => "MINISTRY DESK",
        _ => "UNKNOWN PLACE",
    }
}

fn simple_directory_user(line: u8) -> (&'static str, &'static str, &'static str) {
    match line {
        0 => ("MARA KESH", "rail clerk", "handles relief-train manifests"),
        1 => (
            "DR. LEYA VARAN",
            "clinic registrar",
            "keeps the night ward ledger",
        ),
        2 => (
            "OMAR SEN",
            "ration clerk",
            "issues household allotment cards",
        ),
        3 => (
            "CAPTAIN OREN VEY",
            "border watch officer",
            "on duty until the dawn bell",
        ),
        4 => (
            "NERI TAL",
            "foundry tenant",
            "repairs small motors after shift",
        ),
        5 => (
            "MIRA DHAL",
            "border courier",
            "carries sealed dispatch satchels",
        ),
        6 => (
            "KAVI ORAN",
            "labour registrar",
            "updates the shift board each morning",
        ),
        7 => (
            "SELA VAR",
            "ministry clerk",
            "files permits in the west cabinet",
        ),
        _ => ("UNKNOWN USER", "unlisted", "no directory note"),
    }
}

fn initial_state(
    required_service_calls: u32,
    required_service_kind: Option<ServiceKind>,
) -> StateOutput {
    StateOutput {
        line_lamps: [false; 12],
        game_phase: GamePhase::Ready,
        run_generation: 0,
        clock: ClockState {
            shift: 1,
            elapsed_seconds: accelerated_clock_seconds(0),
        },
        speaker_active: false,
        interference_level: 0,
        tap_bridge_audio_active: false,
        tuning: TuningState::default(),
        directory_pages: directory_pages([0, 0, 0, 1]),
        printer_output: initial_printer_output(required_service_calls, required_service_kind),
        call: None,
        calls: Vec::new(),
        service_call: None,
        tap_bridge_monitoring: None,
        shift: ShiftStatus {
            number: 1,
            phase: ShiftPhase::Ready,
            active_call_count: 0,
            completed_routings: 0,
            required_service_calls,
            completed_service_calls: 0,
            service_errors: 0,
            service_error_counts: Vec::new(),
        },
        debug: OutputDebug {
            messages: vec![BackendDiagnostic {
                code: "backend_ready".to_string(),
                message: "accepting Cabinet Frontend input".to_string(),
            }],
        },
    }
}

fn initial_printer_output(
    required_service_calls: u32,
    required_service_kind: Option<ServiceKind>,
) -> Vec<PrinterEntry> {
    let mut entries = vec![PrinterEntry {
        entry_id: 1,
        text: "SHIFT 1 START\nPROVINCIAL EXCHANGE READY // SHIFT CLOCK 08:00-16:00".to_string(),
    }];
    if required_service_calls > 0
        && let Some(service) = required_service_kind
    {
        entries.push(PrinterEntry {
            entry_id: 2,
            text: format!(
                "SERVICE RULE // {} REQUIRED THIS SHIFT",
                service_label(service)
            ),
        });
    }
    entries
}

fn directory_pages(digits: [u8; 4]) -> Vec<DirectoryPage> {
    let id = digits
        .iter()
        .fold(0_u16, |value, digit| value * 10 + *digit as u16);
    let record = match id {
        1 => Some((
            "TAREN KESH",
            "RAIL DISPATCH",
            "Railway dispatcher",
            "Rail service",
            "Public dispatch desk",
        )),
        2 => Some((
            "VIRA DHAL",
            "RECORDS OFFICE",
            "Factory records clerk",
            "Relief records",
            "Public records contact",
        )),
        3 => Some((
            "DR. LEYA VARAN",
            "KHARAD CLINIC",
            "Emergency physician",
            "Emergency medical service",
            "Clinic duty line",
        )),
        4 => Some((
            "CAPTAIN OREN VEY",
            "BORDER POST",
            "State Protection Directorate",
            "Border security",
            "Public authority contact",
        )),
        5 => Some((
            "NERI TAL",
            "SIGNAL BOX",
            "Railway signal operator",
            "Rail service",
            "Signal maintenance desk",
        )),
        _ => None,
    };

    match record {
        Some((heading, listing, role, affiliation, note)) => vec![
            DirectoryPage {
                page_number: 1,
                heading: heading.to_string(),
                lines: vec![
                    format!("SUBSCRIBER ID {id:04}"),
                    format!("LINE LISTING // {listing}"),
                    role.to_string(),
                ],
            },
            DirectoryPage {
                page_number: 2,
                heading: "PROVINCIAL EXCHANGE".to_string(),
                lines: vec![
                    format!("AFFILIATION // {affiliation}"),
                    format!("PUBLIC NOTE // {note}"),
                ],
            },
        ],
        None => vec![DirectoryPage {
            page_number: 1,
            heading: "NO RECORD".to_string(),
            lines: vec![
                format!("SUBSCRIBER ID {id:04}"),
                "CHECK DIRECTORY SELECTION".to_string(),
            ],
        }],
    }
}

fn north_directory_pages(digits: [u8; 4]) -> Vec<DirectoryPage> {
    let id = directory_id(digits);
    let record = match id {
        1 => Some((0, "ANIKA ROY", "CENTRAL HOSPITAL", "Hospital nurse")),
        2 => Some((1, "NAYAN BORO", "SECRETARIAT", "Government office")),
        3 => Some((
            2,
            "INSPECTOR RAKESH NAHAL",
            "HOME AFFAIRS",
            "Ministry investigator",
        )),
        4 => Some((3, "CAPTAIN VARO", "CANTONMENT", "Army officer")),
        5 => Some((4, "NAYAN BORO", "RADIO", "Radio office")),
        6 => Some((5, "MEERA TAL", "EMBASSY", "Embassy contact")),
        7 => Some((6, "TOMAS VALE", "HOTEL", "Hotel desk")),
        8 => Some((9, "ASHA SEN", "SHAPLA APARTMENTS", "Resident")),
        9 => Some((11, "LALEH MIR", "CENTRAL STATION", "Station contact")),
        _ => None,
    };
    match record {
        Some((line, heading, listing, role)) => vec![
            DirectoryPage {
                page_number: 1,
                heading: heading.to_string(),
                lines: vec![
                    format!("SUBSCRIBER ID {id:04}"),
                    format!("LINE LISTING // {listing}"),
                    format!("LINE NUMBER // {line}"),
                    role.to_string(),
                ],
            },
            DirectoryPage {
                page_number: 2,
                heading: "PROVINCIAL EXCHANGE".to_string(),
                lines: vec!["NORTH NEELADESH DIRECTORY".to_string()],
            },
        ],
        None => vec![DirectoryPage {
            page_number: 1,
            heading: "NO RECORD".to_string(),
            lines: vec![
                format!("SUBSCRIBER ID {id:04}"),
                "CHECK DIRECTORY SELECTION".to_string(),
            ],
        }],
    }
}

fn simple_directory_pages(digits: [u8; 4]) -> Vec<DirectoryPage> {
    let id = directory_id(digits);
    let record = (id < u16::from(SIMPLE_SUBSCRIBER_LINES)).then(|| simple_place_for_line(id as u8));
    match record {
        Some(place) => vec![DirectoryPage {
            page_number: 1,
            heading: place.to_string(),
            lines: {
                let (name, role, note) = simple_directory_user(id as u8);
                vec![
                    format!("SUBSCRIBER ID {id:04}"),
                    format!("USER // {name}"),
                    format!("ROLE // {role}"),
                    format!("NOTE // {note}"),
                    format!("DESTINATION // {place}"),
                ]
            },
        }],
        None => vec![DirectoryPage {
            page_number: 1,
            heading: "NO RECORD".to_string(),
            lines: vec![
                format!("SUBSCRIBER ID {id:04}"),
                "SELECT A LINE FROM 0000 THROUGH 0007".to_string(),
            ],
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crank_lights_every_subscriber_connected_to_ring_generator() {
        let mut lamps = [false; 12];
        let cords = vec![
            CordConnection {
                first: PortId::Subscriber(3),
                second: PortId::RingGenerator,
            },
            CordConnection {
                first: PortId::RingGenerator,
                second: PortId::Subscriber(11),
            },
        ];

        light_ring_generator_lines(&mut lamps, &cords, true);

        assert!(lamps[3]);
        assert!(lamps[11]);
        assert_eq!(lamps.iter().filter(|lamp| **lamp).count(), 2);
    }

    #[test]
    fn ring_generator_lamps_stay_dark_without_a_new_crank_timestamp() {
        let mut lamps = [false; 12];
        let cords = vec![CordConnection {
            first: PortId::Subscriber(3),
            second: PortId::RingGenerator,
        }];

        light_ring_generator_lines(&mut lamps, &cords, false);

        assert!(!lamps[3]);
    }

    #[test]
    fn voice_audio_datagrams_are_sent_at_realtime_rate() {
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let peer = receiver.local_addr().unwrap();
        let packet = RtpL16Packet {
            marker: false,
            sequence: 0,
            timestamp: 0,
            ssrc: 1,
            samples: vec![1; exchange_protocol::VOICE_AUDIO_PACKET_SAMPLES],
        }
        .encode();
        let started = Instant::now();
        let mut next_audio_send = None;

        send_voice_datagrams(
            &sender,
            peer,
            vec![packet.clone(), packet.clone(), packet],
            &mut next_audio_send,
        )
        .unwrap();

        assert!(started.elapsed() >= Duration::from_millis(30));
        let mut buffer = [0_u8; 2_000];
        for _ in 0..3 {
            receiver.recv(&mut buffer).unwrap();
        }
    }

    #[test]
    fn voice_debug_retains_conversation_outputs_and_audio() {
        let backend = Arc::new(Mutex::new(Backend::new_north_neeladesh()));
        let ready = exchange_protocol::VoiceStatusMessage {
            protocol_version: exchange_protocol::VOICE_PROTOCOL_VERSION,
            session_id: 7,
            turn_id: 3,
            state_revision: 2,
            status: VoiceStatus::Ready,
            transcript: None,
            response_text: None,
            error: None,
        };
        let input = exchange_protocol::VoiceInputAudioMessage {
            protocol_version: exchange_protocol::VOICE_PROTOCOL_VERSION,
            session_id: 7,
            turn_id: 3,
            state_revision: 2,
            chunk_index: 0,
            complete: true,
            samples: vec![10, 20, 30],
        };
        {
            let mut state = backend.lock().unwrap();
            assert!(
                state
                    .apply_voice_datagram(&exchange_protocol::encode_voice_status(&ready).unwrap())
            );
            assert!(state.apply_voice_datagram(
                &exchange_protocol::encode_voice_input_audio(&input).unwrap()
            ));
            let (_, _, _, _, _, generation, conversation_id) = state.take_voice_input().unwrap();
            drop(state);

            let mut output = BackendVoiceOutput {
                backend: Arc::clone(&backend),
                generation,
                conversation_id,
            };
            output
                .status(exchange_protocol::VoiceStatusMessage {
                    status: VoiceStatus::Playing,
                    transcript: Some("heard words".to_string()),
                    response_text: Some("spoken response".to_string()),
                    ..ready.clone()
                })
                .unwrap();
            output
                .audio(RtpL16Packet {
                    marker: true,
                    sequence: 0,
                    timestamp: 0,
                    ssrc: 7,
                    samples: vec![40, 50],
                })
                .unwrap();
            let partial_audio =
                backend
                    .lock()
                    .unwrap()
                    .apply_debug_command(DebugCommand::GetVoiceAudio {
                        conversation_id,
                        kind: exchange_protocol::DebugAudioKind::Tts,
                    });
            assert!(!partial_audio.accepted);
            output
                .status(exchange_protocol::VoiceStatusMessage {
                    status: VoiceStatus::Completed,
                    ..ready
                })
                .unwrap();
        }

        let snapshot = backend.lock().unwrap().debug_snapshot();
        let conversation = &snapshot.voice.conversations[0];
        assert_eq!(conversation.transcript.as_deref(), Some("heard words"));
        assert_eq!(
            conversation.response_text.as_deref(),
            Some("spoken response")
        );
        assert_eq!(conversation.captured_samples, 3);
        assert_eq!(conversation.tts_samples, 2);

        let audio = backend
            .lock()
            .unwrap()
            .apply_debug_command(DebugCommand::GetVoiceAudio {
                conversation_id: conversation.id,
                kind: exchange_protocol::DebugAudioKind::Tts,
            });
        assert!(audio.accepted);
        assert_eq!(audio.audio.unwrap().samples, vec![40, 50]);
    }
}
