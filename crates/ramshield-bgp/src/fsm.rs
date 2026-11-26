use crate::Result;

/// BGP FSM states per RFC 4271 Section 8
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgpState {
    Idle,
    Connect,
    Active,
    OpenSent,
    OpenConfirm,
    Established,
}

/// BGP FSM session attributes (mandatory per RFC 4271)
#[derive(Debug, Clone)]
pub struct BgpSessionAttrs {
    pub state: BgpState,
    pub connect_retry_counter: u32,
    pub connect_retry_timer: u32, // seconds
    pub connect_retry_time: u32,  // seconds until next retry
    pub hold_timer: u32,          // seconds
    pub hold_time: u16,           // negotiated hold time
    pub keepalive_timer: u32,     // seconds
    pub keepalive_time: u16,      // negotiated keepalive interval
}

/// BGP FSM events
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgpEvent {
    ManualStart,
    ManualStop,
    AutoStart,
    AutoStop,
    TcpConnect,
    TcpConnectFail,
    TcpConnectionConfirmed,
    OpenSent,
    OpenReceived,
    OpenConfirmReceived,
    KeepaliveReceived,
    NotificationReceived,
    HoldTimerExpired,
    KeepaliveTimerExpired,
    ConnectRetryTimerExpired,
    DelayOpenTimerExpired,
}

/// Transition action to take after state change
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionAction {
    None,
    SendOpen,
    SendKeepalive,
    SendNotification(u8, u8),
    EstablishTcp,
    CloseTcp,
    IncrementRetry,
    ResetRetry,
    StartHoldTimer,
    StartKeepaliveTimer,
    StartConnectRetryTimer,
    ClearConnectRetryTimer,
    ClearHoldTimer,
    ClearKeepaliveTimer,
}

/// BGP Finite State Machine
#[derive(Debug)]
pub struct BgpFsm {
    pub attrs: BgpSessionAttrs,
    pub local_asn: u16,
    pub peer_asn: u16,
    pub peer_ip: std::net::IpAddr,
    pub router_id: u32,
    pub hold_time: u16,
    pub keepalive_time: u16,
}

impl BgpFsm {
    pub fn new(local_asn: u16, peer_asn: u16, peer_ip: std::net::IpAddr, router_id: u32) -> Self {
        let hold_time = 90;
        let keepalive_time = hold_time / 3;
        BgpFsm {
            attrs: BgpSessionAttrs {
                state: BgpState::Idle,
                connect_retry_counter: 0,
                connect_retry_timer: 120,
                connect_retry_time: 0,
                hold_timer: 0,
                hold_time,
                keepalive_timer: 0,
                keepalive_time,
            },
            local_asn,
            peer_asn,
            peer_ip,
            router_id,
            hold_time,
            keepalive_time,
        }
    }

    /// Process an event and return action to take
    pub fn process_event(&mut self, event: BgpEvent) -> Result<Vec<TransitionAction>> {
        let state = self.attrs.state;
        let mut actions = Vec::new();

        match (state, event) {
            // Idle state
            (BgpState::Idle, BgpEvent::ManualStart) | (BgpState::Idle, BgpEvent::AutoStart) => {
                self.attrs.state = BgpState::Connect;
                actions.push(TransitionAction::StartConnectRetryTimer);
                actions.push(TransitionAction::EstablishTcp);
            }

            // Connect state
            (BgpState::Connect, BgpEvent::TcpConnectionConfirmed) => {
                self.attrs.state = BgpState::OpenSent;
                actions.push(TransitionAction::SendOpen);
                actions.push(TransitionAction::ClearConnectRetryTimer);
                actions.push(TransitionAction::StartHoldTimer);
            }
            (BgpState::Connect, BgpEvent::TcpConnectFail) => {
                actions.push(TransitionAction::IncrementRetry);
                actions.push(TransitionAction::StartConnectRetryTimer);
            }
            (BgpState::Connect, BgpEvent::ConnectRetryTimerExpired) => {
                // RFC 4271 §8.2.2: a retry initiated from Connect enters Active.
                self.attrs.state = BgpState::Active;
                actions.push(TransitionAction::EstablishTcp);
                actions.push(TransitionAction::StartConnectRetryTimer);
            }

            // Active state
            (BgpState::Active, BgpEvent::TcpConnectionConfirmed) => {
                self.attrs.state = BgpState::OpenSent;
                actions.push(TransitionAction::SendOpen);
                actions.push(TransitionAction::ClearConnectRetryTimer);
                actions.push(TransitionAction::StartHoldTimer);
            }
            (BgpState::Active, BgpEvent::TcpConnectFail)
            | (BgpState::Active, BgpEvent::ConnectRetryTimerExpired) => {
                actions.push(TransitionAction::EstablishTcp);
                actions.push(TransitionAction::StartConnectRetryTimer);
            }
            (BgpState::Active, BgpEvent::NotificationReceived) => {
                self.attrs.state = BgpState::Idle;
                actions.push(TransitionAction::CloseTcp);
            }

            // OpenSent state
            (BgpState::OpenSent, BgpEvent::OpenReceived) => {
                self.attrs.state = BgpState::OpenConfirm;
                actions.push(TransitionAction::SendKeepalive);
            }
            (BgpState::OpenSent, BgpEvent::NotificationReceived) => {
                self.attrs.state = BgpState::Idle;
                actions.push(TransitionAction::CloseTcp);
            }
            (BgpState::OpenSent, BgpEvent::HoldTimerExpired) => {
                // RFC 4271 §8.2.3: hold expiry before OPEN is confirmed is
                // reported with Hold Timer Expired, then the TCP session closes.
                self.attrs.state = BgpState::Idle;
                actions.push(TransitionAction::SendNotification(4, 0));
                actions.push(TransitionAction::CloseTcp);
            }

            // OpenConfirm state
            (BgpState::OpenConfirm, BgpEvent::KeepaliveReceived) => {
                self.attrs.state = BgpState::Established;
                actions.push(TransitionAction::StartKeepaliveTimer);
            }
            (BgpState::OpenConfirm, BgpEvent::NotificationReceived) => {
                self.attrs.state = BgpState::Idle;
                actions.push(TransitionAction::CloseTcp);
            }
            (BgpState::OpenConfirm, BgpEvent::HoldTimerExpired) => {
                self.attrs.state = BgpState::Idle;
                actions.push(TransitionAction::SendNotification(4, 0));
                actions.push(TransitionAction::CloseTcp);
            }

            // Established state
            (BgpState::Established, BgpEvent::KeepaliveReceived) => {
                // Reset hold timer on valid KEEPALIVE
                actions.push(TransitionAction::StartHoldTimer);
            }
            (BgpState::Established, BgpEvent::HoldTimerExpired) => {
                self.attrs.state = BgpState::Idle;
                actions.push(TransitionAction::SendNotification(4, 0)); // Hold Timer Expired
                actions.push(TransitionAction::CloseTcp);
            }
            (BgpState::Established, BgpEvent::NotificationReceived) => {
                self.attrs.state = BgpState::Idle;
                actions.push(TransitionAction::CloseTcp);
            }

            // Any state -> Idle on manual stop
            (_, BgpEvent::ManualStop) | (_, BgpEvent::AutoStop) if state != BgpState::Idle => {
                actions.push(TransitionAction::SendNotification(6, 0)); // Cease
                actions.push(TransitionAction::CloseTcp);
                self.attrs.state = BgpState::Idle;
            }

            _ => {}
        }

        Ok(actions)
    }

    /// Get current state
    pub fn state(&self) -> BgpState {
        self.attrs.state
    }

    /// Check if session is established
    pub fn is_established(&self) -> bool {
        self.attrs.state == BgpState::Established
    }
}
