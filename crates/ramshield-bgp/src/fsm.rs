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
                // RFC 4271 §8.2.2: a failed TCP attempt releases the
                // connection resources and returns the peer FSM to Idle.
                self.attrs.state = BgpState::Idle;
                actions.push(TransitionAction::IncrementRetry);
                actions.push(TransitionAction::ClearConnectRetryTimer);
                actions.push(TransitionAction::CloseTcp);
            }
            (BgpState::Connect, BgpEvent::ConnectRetryTimerExpired) => {
                // RFC 4271 §8.2.2: retry from Connect initiates another TCP
                // attempt while remaining in Connect.
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
            (BgpState::Active, BgpEvent::TcpConnectFail) => {
                // RFC 4271 §8.2.2: restart retry timing, release resources,
                // and return to Idle after the TCP connection fails.
                self.attrs.state = BgpState::Idle;
                actions.push(TransitionAction::IncrementRetry);
                actions.push(TransitionAction::StartConnectRetryTimer);
                actions.push(TransitionAction::CloseTcp);
            }
            (BgpState::Active, BgpEvent::ConnectRetryTimerExpired) => {
                // RFC 4271 §8.2.2: retry from Active returns to Connect and
                // initiates a new outbound TCP connection.
                self.attrs.state = BgpState::Connect;
                actions.push(TransitionAction::EstablishTcp);
                actions.push(TransitionAction::StartConnectRetryTimer);
            }
            (BgpState::OpenSent, BgpEvent::TcpConnectFail) => {
                // RFC 4271 §8.2.2: a failed connection in OpenSent returns
                // the FSM to Active and resumes listening/retry timing.
                self.attrs.state = BgpState::Active;
                actions.push(TransitionAction::StartConnectRetryTimer);
                actions.push(TransitionAction::CloseTcp);
            }

            (BgpState::OpenConfirm, BgpEvent::TcpConnectFail)
            | (BgpState::Established, BgpEvent::TcpConnectFail) => {
                // RFC 4271 §8.2.2: a transport failure after OPEN confirmation
                // tears down the session and returns it to Idle.
                self.attrs.state = BgpState::Idle;
                actions.push(TransitionAction::IncrementRetry);
                actions.push(TransitionAction::ClearConnectRetryTimer);
                actions.push(TransitionAction::CloseTcp);
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

#[cfg(test)]
mod retry_transition_tests {
    use super::*;

    fn fsm() -> BgpFsm {
        BgpFsm::new(65001, 65002, "192.0.2.2".parse().unwrap(), 0x0A00_0001)
    }

    #[test]
    fn connect_retry_expiry_retries_without_entering_active() {
        let mut fsm = fsm();
        fsm.process_event(BgpEvent::ManualStart).unwrap();
        let actions = fsm
            .process_event(BgpEvent::ConnectRetryTimerExpired)
            .unwrap();
        assert_eq!(fsm.state(), BgpState::Connect);
        assert!(actions.contains(&TransitionAction::EstablishTcp));
        assert!(actions.contains(&TransitionAction::StartConnectRetryTimer));
    }

    #[test]
    fn active_retry_expiry_returns_to_connect() {
        let mut fsm = fsm();
        fsm.attrs.state = BgpState::Active;
        let actions = fsm
            .process_event(BgpEvent::ConnectRetryTimerExpired)
            .unwrap();
        assert_eq!(fsm.state(), BgpState::Connect);
        assert!(actions.contains(&TransitionAction::EstablishTcp));
    }

    #[test]
    fn connect_tcp_failure_returns_to_idle_and_clears_retry_timer() {
        let mut fsm = fsm();
        fsm.process_event(BgpEvent::ManualStart).unwrap();
        let actions = fsm.process_event(BgpEvent::TcpConnectFail).unwrap();
        assert_eq!(fsm.state(), BgpState::Idle);
        assert!(actions.contains(&TransitionAction::IncrementRetry));
        assert!(actions.contains(&TransitionAction::ClearConnectRetryTimer));
        assert!(actions.contains(&TransitionAction::CloseTcp));
    }

    #[test]
    fn active_tcp_failure_returns_to_idle_and_restarts_retry_timer() {
        let mut fsm = fsm();
        fsm.attrs.state = BgpState::Active;
        let actions = fsm.process_event(BgpEvent::TcpConnectFail).unwrap();
        assert_eq!(fsm.state(), BgpState::Idle);
        assert!(actions.contains(&TransitionAction::IncrementRetry));
        assert!(actions.contains(&TransitionAction::StartConnectRetryTimer));
        assert!(actions.contains(&TransitionAction::CloseTcp));
    }

    #[test]
    fn tcp_failure_after_open_returns_to_active() {
        let mut fsm = fsm();
        fsm.attrs.state = BgpState::OpenSent;
        let actions = fsm.process_event(BgpEvent::TcpConnectFail).unwrap();
        assert_eq!(fsm.state(), BgpState::Active);
        assert!(actions.contains(&TransitionAction::StartConnectRetryTimer));
        assert!(actions.contains(&TransitionAction::CloseTcp));
    }

    #[test]
    fn tcp_failure_after_open_confirmation_returns_to_idle() {
        for state in [BgpState::OpenConfirm, BgpState::Established] {
            let mut fsm = fsm();
            fsm.attrs.state = state;
            let actions = fsm.process_event(BgpEvent::TcpConnectFail).unwrap();
            assert_eq!(fsm.state(), BgpState::Idle);
            assert!(actions.contains(&TransitionAction::IncrementRetry));
            assert!(actions.contains(&TransitionAction::ClearConnectRetryTimer));
            assert!(actions.contains(&TransitionAction::CloseTcp));
        }
    }
}
