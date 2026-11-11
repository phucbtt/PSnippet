//! Segmented-composer session FSM (spec §6A.6). Sans-IO and RAM-only.

use zs_core::{RenderSegment, render_plain_text_preview};

use crate::policy::{MentionMode, MentionVerification};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComposeState {
    Idle,
    TriggerCaptured,
    AwaitFields,
    CancelledUnchanged,
    RevalidateTarget,
    CopyOnlySafe,
    RemoveTrigger,
    InsertSegment,
    AwaitMentionSelection,
    ManualAssist,
    DraftCompleted,
    AbortedSafe,
    AbortedPartial,
}

impl ComposeState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::CancelledUnchanged
                | Self::CopyOnlySafe
                | Self::DraftCompleted
                | Self::AbortedSafe
                | Self::AbortedPartial
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    TriggerMatched,
    ContextRejected,
    OpenForm,
    Cancel,
    ConfirmValid(Vec<RenderSegment>),
    TargetValid,
    /// Chat/window changed or could not be verified.
    TargetInvalid,
    TriggerRemoved,
    TriggerRemoveFailed,
    SegmentInjected,
    InjectFailed,
    /// User picked the member in Zalo and pressed Resume. `same_context` is the result of re-validation.
    MentionUserConfirmed {
        same_context: bool,
    },
    /// M2 only. Rejected unless the session mode is AutoVerified.
    MentionAutoVerified {
        same_context: bool,
    },
    /// Dropdown missing, ambiguous, stale: fall back to manual help.
    MentionNeedsManual,
    TargetChanged,
    Timeout,
    CancelRemaining,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    TargetChanged,
    TriggerUnsafe,
    MentionNotConfirmed,
    MentionUnavailable,
    SessionTimeout,
    InjectFailed,
}

impl ErrorCode {
    pub fn code(self) -> &'static str {
        match self {
            Self::TargetChanged => "E_TARGET_CHANGED",
            Self::TriggerUnsafe => "E_TRIGGER_UNSAFE",
            Self::MentionNotConfirmed => "E_MENTION_NOT_CONFIRMED",
            Self::MentionUnavailable => "E_MENTION_UNAVAILABLE",
            Self::SessionTimeout => "E_SESSION_TIMEOUT",
            Self::InjectFailed => "E_INPUT_COLLISION",
        }
    }
    pub fn message_vi(self) -> &'static str {
        match self {
            Self::TargetChanged => "Đã thay đổi cuộc trò chuyện; không tiếp tục chèn",
            Self::TriggerUnsafe => "Vui lòng xóa từ khóa trong Zalo rồi dán thủ công",
            Self::MentionNotConfirmed => "Chưa xác nhận đã chọn người",
            Self::MentionUnavailable => "Zalo chưa hiển thị danh sách @; hãy chọn thủ công",
            Self::SessionTimeout => "Phiên chèn đã hết hiệu lực",
            Self::InjectFailed => "Đã dừng để tránh ghi đè nội dung",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// `n` of `total` mentions awaiting selection (1-based n).
    WaitingMention {
        n: usize,
        total: usize,
    },
    ManualAssist(ErrorCode),
    /// Draft prepared. Never means "sent".
    DraftReady {
        mentions: usize,
        verification: MentionVerification,
    },
    Aborted(ErrorCode),
}

/// Effects the platform layer must perform. There is intentionally no Send/Enter action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    OpenForm,
    RevalidateTarget,
    RemoveTrigger,
    InjectText(String),
    /// Begin the mention at the caret. In M0 this only shows instructions; M1 may type `@query`.
    RequestMention {
        query: String,
        index: usize,
        total: usize,
    },
    /// Put text on the clipboard for manual paste. Mentions in it are illustrative `@name` text only.
    CopyText {
        text: String,
        mentions_not_preserved: bool,
    },
    Show(Status),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("illegal transition: {event:?} in state {state:?}")]
pub struct IllegalTransition {
    pub state: ComposeState,
    pub event: Event,
}

pub struct ComposeSession {
    pub id: uuid::Uuid,
    state: ComposeState,
    mode: MentionMode,
    segments: Vec<RenderSegment>,
    next_index: usize,
    selected_mentions: usize,
}

impl ComposeSession {
    pub fn new(mode: MentionMode) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            state: ComposeState::Idle,
            mode,
            segments: Vec::new(),
            next_index: 0,
            selected_mentions: 0,
        }
    }

    pub fn state(&self) -> ComposeState {
        self.state
    }
    pub fn selected_mentions(&self) -> usize {
        self.selected_mentions
    }
    fn total_mentions(&self) -> usize {
        self.segments
            .iter()
            .filter(|s| matches!(s, RenderSegment::Mention { .. }))
            .count()
    }

    /// Illustrative text of everything not yet inserted (for Copy-only / abort recovery).
    pub fn remaining_text(&self) -> String {
        render_plain_text_preview(&self.segments[self.next_index.min(self.segments.len())..])
    }

    fn full_text(&self) -> String {
        render_plain_text_preview(&self.segments)
    }

    fn goto(&mut self, s: ComposeState) {
        self.state = s;
    }

    /// Walk to the next segment and emit its action.
    fn advance(&mut self) -> Vec<Action> {
        match self.segments.get(self.next_index) {
            None => {
                self.goto(ComposeState::DraftCompleted);
                vec![Action::Show(Status::DraftReady {
                    mentions: self.selected_mentions,
                    // Resume is a user attestation, never machine proof (§6A.10).
                    verification: if self.selected_mentions == 0 {
                        MentionVerification::Unverifiable
                    } else if self.mode == MentionMode::AutoVerified {
                        MentionVerification::VerifiedRichMention
                    } else {
                        MentionVerification::UserConfirmed
                    },
                })]
            }
            Some(RenderSegment::Text(t)) => {
                let t = t.clone();
                self.goto(ComposeState::InsertSegment);
                vec![Action::InjectText(t)]
            }
            Some(RenderSegment::Mention { query, .. }) => {
                let query = query.clone();
                self.goto(ComposeState::AwaitMentionSelection);
                vec![
                    Action::RequestMention {
                        query,
                        index: self.selected_mentions,
                        total: self.total_mentions(),
                    },
                    Action::Show(Status::WaitingMention {
                        n: self.selected_mentions + 1,
                        total: self.total_mentions(),
                    }),
                ]
            }
        }
    }

    fn abort_partial(&mut self, code: ErrorCode) -> Vec<Action> {
        // Never roll back text already inserted (§6A.6): only offer the remainder.
        let remaining = self.remaining_text();
        self.goto(ComposeState::AbortedPartial);
        let mut v = vec![Action::Show(Status::Aborted(code))];
        if !remaining.is_empty() {
            v.push(Action::CopyText {
                text: remaining,
                mentions_not_preserved: true,
            });
        }
        v
    }

    pub fn handle(&mut self, ev: Event) -> Result<Vec<Action>, IllegalTransition> {
        use ComposeState as S;
        use Event as E;
        let illegal = |s: &Self, ev: Event| {
            Err(IllegalTransition {
                state: s.state,
                event: ev,
            })
        };

        let actions = match (self.state, ev) {
            (S::Idle, E::TriggerMatched) => {
                self.goto(S::TriggerCaptured);
                vec![]
            }
            (S::TriggerCaptured, E::ContextRejected) => {
                self.goto(S::AbortedSafe);
                vec![]
            }
            (S::TriggerCaptured, E::OpenForm) => {
                self.goto(S::AwaitFields);
                vec![Action::OpenForm]
            }
            // Cancel before anything touched Zalo: trigger text stays exactly as typed.
            (S::AwaitFields, E::Cancel) => {
                self.goto(S::CancelledUnchanged);
                vec![]
            }
            (S::AwaitFields, E::ConfirmValid(segs)) => {
                self.segments = segs;
                self.next_index = 0;
                self.selected_mentions = 0;
                self.goto(S::RevalidateTarget);
                vec![Action::RevalidateTarget]
            }
            (S::RevalidateTarget, E::TargetValid) => {
                self.goto(S::RemoveTrigger);
                vec![Action::RemoveTrigger]
            }
            (S::RevalidateTarget, E::TargetInvalid) => {
                self.goto(S::CopyOnlySafe);
                vec![
                    Action::Show(Status::Aborted(ErrorCode::TargetChanged)),
                    Action::CopyText {
                        text: self.full_text(),
                        mentions_not_preserved: self.total_mentions() > 0,
                    },
                ]
            }
            (S::RemoveTrigger, E::TriggerRemoved) => self.advance(),
            (S::RemoveTrigger, E::TriggerRemoveFailed) => {
                // Nothing inserted yet; never retry blind backspaces.
                self.goto(S::CopyOnlySafe);
                vec![
                    Action::Show(Status::Aborted(ErrorCode::TriggerUnsafe)),
                    Action::CopyText {
                        text: self.full_text(),
                        mentions_not_preserved: self.total_mentions() > 0,
                    },
                ]
            }
            (S::InsertSegment, E::SegmentInjected) => {
                self.next_index += 1;
                self.advance()
            }
            (S::InsertSegment, E::InjectFailed) => self.abort_partial(ErrorCode::InjectFailed),

            (
                S::AwaitMentionSelection | S::ManualAssist,
                E::MentionUserConfirmed { same_context },
            ) => {
                if same_context {
                    self.selected_mentions += 1;
                    self.next_index += 1;
                    self.advance()
                } else {
                    self.abort_partial(ErrorCode::TargetChanged)
                }
            }
            (S::AwaitMentionSelection, E::MentionAutoVerified { same_context })
                if self.mode == MentionMode::AutoVerified =>
            {
                if same_context {
                    self.selected_mentions += 1;
                    self.next_index += 1;
                    self.advance()
                } else {
                    self.abort_partial(ErrorCode::TargetChanged)
                }
            }
            (S::AwaitMentionSelection, E::MentionNeedsManual) => {
                self.goto(S::ManualAssist);
                vec![Action::Show(Status::ManualAssist(
                    ErrorCode::MentionUnavailable,
                ))]
            }
            (S::AwaitMentionSelection | S::ManualAssist, E::TargetChanged) => {
                self.abort_partial(ErrorCode::TargetChanged)
            }
            (S::AwaitMentionSelection | S::ManualAssist, E::Timeout) => {
                self.abort_partial(ErrorCode::SessionTimeout)
            }
            (S::AwaitMentionSelection | S::ManualAssist, E::CancelRemaining) => {
                self.abort_partial(ErrorCode::MentionNotConfirmed)
            }
            // Focus changed mid-insert: stop, keep what is there.
            (S::InsertSegment, E::TargetChanged) => self.abort_partial(ErrorCode::TargetChanged),

            (_, ev) => return illegal(self, ev),
        };
        Ok(actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zs_core::{FieldKey, FieldValues, MentionChoice, Template};

    fn plan(body: &str, vals: &[(&str, &str)]) -> Vec<RenderSegment> {
        let t = Template::parse(body).unwrap();
        let mut v = FieldValues::new();
        for (k, x) in vals {
            v.set(&FieldKey::new(k), *x);
        }
        t.render(&v).unwrap()
    }

    /// Contract-test editor: models a rich composer holding real mention tokens (§12A.1 #2).
    /// It proves ordering/logic only, not that Zalo behaves this way.
    #[derive(Debug, PartialEq, Eq, Clone)]
    enum Item {
        Text(String),
        Mention(String),
    }

    struct MockRichEditor {
        items: Vec<Item>,
        trigger_present: bool,
        sent: bool, // can never be set: Action has no Send
    }

    /// Drive a session to completion, simulating a user who picks each requested member.
    fn drive(s: &mut ComposeSession, ed: &mut MockRichEditor, segs: Vec<RenderSegment>) {
        s.handle(Event::TriggerMatched).unwrap();
        assert_eq!(s.handle(Event::OpenForm).unwrap(), vec![Action::OpenForm]);
        let mut work: Vec<Action> = s.handle(Event::ConfirmValid(segs)).unwrap();
        while let Some(a) = work.pop() {
            let next = match a {
                Action::RevalidateTarget => s.handle(Event::TargetValid).unwrap(),
                Action::RemoveTrigger => {
                    ed.trigger_present = false;
                    s.handle(Event::TriggerRemoved).unwrap()
                }
                Action::InjectText(t) => {
                    ed.items.push(Item::Text(t));
                    s.handle(Event::SegmentInjected).unwrap()
                }
                Action::RequestMention { query, .. } => {
                    ed.items.push(Item::Mention(query));
                    s.handle(Event::MentionUserConfirmed { same_context: true })
                        .unwrap()
                }
                Action::Show(_) | Action::OpenForm | Action::CopyText { .. } => vec![],
            };
            work.extend(next.into_iter().rev());
        }
    }

    #[test]
    fn two_mentions_insert_in_order_text_mention_text_mention_text() {
        // MN-07
        let segs = plan(
            "Cập nhật [A:mention] và [B:mention]: [N]",
            &[("A", "Hiền"), ("B", "Lan"), ("N", "ok")],
        );
        let mut s = ComposeSession::new(MentionMode::Manual);
        let mut ed = MockRichEditor {
            items: vec![],
            trigger_present: true,
            sent: false,
        };
        drive(&mut s, &mut ed, segs);
        assert_eq!(s.state(), ComposeState::DraftCompleted);
        assert_eq!(
            ed.items,
            vec![
                Item::Text("Cập nhật ".into()),
                Item::Mention("Hiền".into()),
                Item::Text(" và ".into()),
                Item::Mention("Lan".into()),
                Item::Text(": ok".into()),
            ]
        );
        assert!(!ed.trigger_present && !ed.sent);
        assert_eq!(s.selected_mentions(), 2);
        // No Text item fakes a mention with "@".
        assert!(
            ed.items
                .iter()
                .all(|i| !matches!(i, Item::Text(t) if t.contains('@')))
        );
    }

    #[test]
    fn mention_first_template_waits_before_any_text() {
        let segs = plan("[A:mention], em đã nhận.", &[("A", "Hiền")]);
        let mut s = ComposeSession::new(MentionMode::Manual);
        s.handle(Event::TriggerMatched).unwrap();
        s.handle(Event::OpenForm).unwrap();
        s.handle(Event::ConfirmValid(segs)).unwrap();
        s.handle(Event::TargetValid).unwrap();
        let acts = s.handle(Event::TriggerRemoved).unwrap();
        assert_eq!(s.state(), ComposeState::AwaitMentionSelection);
        assert!(matches!(acts[0], Action::RequestMention { .. }));
        assert!(!acts.iter().any(|a| matches!(a, Action::InjectText(_))));
    }

    #[test]
    fn cancel_before_confirm_changes_nothing() {
        // MN-09
        let mut s = ComposeSession::new(MentionMode::Manual);
        s.handle(Event::TriggerMatched).unwrap();
        s.handle(Event::OpenForm).unwrap();
        let acts = s.handle(Event::Cancel).unwrap();
        assert!(acts.is_empty());
        assert_eq!(s.state(), ComposeState::CancelledUnchanged);
    }

    #[test]
    fn target_changed_at_confirm_goes_copy_only_without_removing_trigger() {
        // MN-11 / IT-03
        let segs = plan("Dạ [A:mention]", &[("A", "Hiền")]);
        let mut s = ComposeSession::new(MentionMode::Manual);
        s.handle(Event::TriggerMatched).unwrap();
        s.handle(Event::OpenForm).unwrap();
        s.handle(Event::ConfirmValid(segs)).unwrap();
        let acts = s.handle(Event::TargetInvalid).unwrap();
        assert_eq!(s.state(), ComposeState::CopyOnlySafe);
        assert!(
            !acts
                .iter()
                .any(|a| matches!(a, Action::RemoveTrigger | Action::InjectText(_)))
        );
        assert!(acts.iter().any(|a| matches!(
            a,
            Action::CopyText {
                mentions_not_preserved: true,
                ..
            }
        )));
    }

    #[test]
    fn trigger_removal_failure_is_not_retried() {
        let segs = plan("x", &[]);
        let mut s = ComposeSession::new(MentionMode::Manual);
        s.handle(Event::TriggerMatched).unwrap();
        s.handle(Event::OpenForm).unwrap();
        s.handle(Event::ConfirmValid(segs)).unwrap();
        s.handle(Event::TargetValid).unwrap();
        let acts = s.handle(Event::TriggerRemoveFailed).unwrap();
        assert_eq!(s.state(), ComposeState::CopyOnlySafe);
        assert!(acts.contains(&Action::Show(Status::Aborted(ErrorCode::TriggerUnsafe))));
        assert!(s.handle(Event::TriggerRemoveFailed).is_err());
    }

    fn at_first_mention() -> ComposeSession {
        let segs = plan(
            "Dạ [A:mention] ok [B:mention] xong",
            &[("A", "Hiền"), ("B", "Lan")],
        );
        let mut s = ComposeSession::new(MentionMode::Manual);
        s.handle(Event::TriggerMatched).unwrap();
        s.handle(Event::OpenForm).unwrap();
        s.handle(Event::ConfirmValid(segs)).unwrap();
        s.handle(Event::TargetValid).unwrap();
        s.handle(Event::TriggerRemoved).unwrap(); // injects "Dạ "
        s.handle(Event::SegmentInjected).unwrap(); // now awaiting mention 1
        assert_eq!(s.state(), ComposeState::AwaitMentionSelection);
        s
    }

    #[test]
    fn chat_change_while_waiting_aborts_and_offers_remainder_only() {
        // MN-12
        let mut s = at_first_mention();
        let acts = s.handle(Event::TargetChanged).unwrap();
        assert_eq!(s.state(), ComposeState::AbortedPartial);
        assert!(!acts.iter().any(|a| matches!(a, Action::InjectText(_))));
        let copy = acts.iter().find_map(|a| {
            if let Action::CopyText { text, .. } = a {
                Some(text.clone())
            } else {
                None
            }
        });
        assert_eq!(copy.unwrap(), "@Hiền ok @Lan xong"); // remainder, not "Dạ " (already in Zalo)
    }

    #[test]
    fn resume_in_changed_chat_does_not_insert_suffix() {
        // MN-39
        let mut s = at_first_mention();
        let acts = s
            .handle(Event::MentionUserConfirmed {
                same_context: false,
            })
            .unwrap();
        assert_eq!(s.state(), ComposeState::AbortedPartial);
        assert!(!acts.iter().any(|a| matches!(a, Action::InjectText(_))));
    }

    #[test]
    fn cancel_after_first_tag_stops_remaining() {
        // MN-10
        let mut s = at_first_mention();
        s.handle(Event::MentionUserConfirmed { same_context: true })
            .unwrap(); // inject " ok "
        s.handle(Event::SegmentInjected).unwrap();
        assert_eq!(s.state(), ComposeState::AwaitMentionSelection);
        let acts = s.handle(Event::CancelRemaining).unwrap();
        assert_eq!(s.state(), ComposeState::AbortedPartial);
        assert!(!acts.iter().any(|a| matches!(a, Action::InjectText(_))));
    }

    #[test]
    fn timeout_and_manual_assist_paths() {
        let mut s = at_first_mention();
        let acts = s.handle(Event::MentionNeedsManual).unwrap();
        assert_eq!(s.state(), ComposeState::ManualAssist);
        assert!(matches!(acts[0], Action::Show(Status::ManualAssist(_))));
        // user can still finish manually
        s.handle(Event::MentionUserConfirmed { same_context: true })
            .unwrap();
        assert_eq!(s.state(), ComposeState::InsertSegment);

        let mut s = at_first_mention();
        s.handle(Event::Timeout).unwrap();
        assert_eq!(s.state(), ComposeState::AbortedPartial); // MN-27
        assert!(
            s.handle(Event::MentionUserConfirmed { same_context: true })
                .is_err()
        ); // stale resume refused
    }

    #[test]
    fn auto_event_rejected_unless_auto_mode() {
        // MN-36
        let mut s = at_first_mention();
        assert!(
            s.handle(Event::MentionAutoVerified { same_context: true })
                .is_err()
        );
        assert_eq!(s.state(), ComposeState::AwaitMentionSelection);
    }

    #[test]
    fn completion_reports_user_confirmed_not_verified() {
        let segs = plan("[A:mention]", &[("A", "Hiền")]);
        let mut s = ComposeSession::new(MentionMode::Manual);
        s.handle(Event::TriggerMatched).unwrap();
        s.handle(Event::OpenForm).unwrap();
        s.handle(Event::ConfirmValid(segs)).unwrap();
        s.handle(Event::TargetValid).unwrap();
        s.handle(Event::TriggerRemoved).unwrap();
        let acts = s
            .handle(Event::MentionUserConfirmed { same_context: true })
            .unwrap();
        assert_eq!(
            acts,
            vec![Action::Show(Status::DraftReady {
                mentions: 1,
                verification: MentionVerification::UserConfirmed
            })]
        );
    }

    #[test]
    fn plain_text_choice_template_has_no_mention_step() {
        let t = Template::parse("Dạ chị [T:mention?].").unwrap();
        let mut v = FieldValues::new();
        let k = FieldKey::new("T");
        v.set(&k, "Hiền");
        v.set_choice(&k, MentionChoice::PlainText);
        let segs = t.render(&v).unwrap();
        let mut s = ComposeSession::new(MentionMode::Manual);
        let mut ed = MockRichEditor {
            items: vec![],
            trigger_present: true,
            sent: false,
        };
        drive(&mut s, &mut ed, segs);
        assert_eq!(ed.items, vec![Item::Text("Dạ chị Hiền.".into())]);
        assert_eq!(s.selected_mentions(), 0);
    }

    #[test]
    fn illegal_events_leave_state_untouched() {
        let mut s = ComposeSession::new(MentionMode::Manual);
        assert!(s.handle(Event::SegmentInjected).is_err());
        assert_eq!(s.state(), ComposeState::Idle);
    }
}
