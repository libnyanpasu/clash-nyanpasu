//! Pure command policy and the deferral decision.
//!
//! Two static questions, both answered from typed data and never from a
//! parameter the caller chose: what is this command allowed to do when its
//! critical part does not apply ([`CommandPolicy`]), and given what the Try
//! actually reported, may the commit still go ahead ([`disposition`]).

use super::impact::{ChangedOwnerInputs, RuntimeImpact};

/// What a command may do when the runtime part of it cannot be applied.
///
/// Assigned by [`policy_for`] from the command class and the classified impact.
/// The frontend cannot pick it: a request must not be able to declare itself
/// exempt from confirming that a core switch took effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandPolicy {
    /// The target has to be confirmed in effect before the source config is
    /// committed. A refused elevation is a plain rejection, not a reason to
    /// keep prompting in the background.
    MustApply,
    /// The desired value may be committed unapplied, but only when every
    /// condition in [`disposition`] holds.
    AllowDeferredWhenSafe,
    /// Nothing to apply and no owner to notify: validate and commit.
    SaveOnly,
    /// Nothing critical to apply, but peripheral owners have a new target.
    /// Commit on validation and let them reconcile afterwards.
    SaveThenNotify,
    /// The user stopped the core. Save the checked target, start nothing, and
    /// run no retry loop against that intent.
    SavedInactive,
}

impl CommandPolicy {
    /// Whether this command may commit a desired value it could not apply.
    ///
    /// Only one policy may. `MustApply` refuses by definition; under the other
    /// three no critical Try was owed in the first place, so a failure from one
    /// is a contradiction rather than a licence to defer.
    pub fn allows_deferral(self) -> bool {
        matches!(self, Self::AllowDeferredWhenSafe)
    }
}

/// What kind of command produced the candidate.
///
/// The split is the one the policy table needs, and it follows the command, not
/// its diff: asking for a specific core, host or profile to be running is a
/// different promise from saving a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandClass {
    /// `select_core`, `set_execution_host`, and profile activation or
    /// deactivation: the user named something that has to be running.
    ExplicitSwitch,
    /// Every other source-config write: clash and overrides parameters, managed
    /// content updates, profile bookkeeping, application settings.
    // Chosen by the domain actors, which move onto the participant in T6.
    #[allow(dead_code)]
    Save,
}

/// Whether the user wants a core running at all.
///
/// A stopped core is an intent, not a failure: an ordinary save must not start
/// one behind the user's back just because it has something to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoreRunIntent {
    Running,
    StoppedByUser,
}

/// The static policy for one candidate.
pub(crate) fn policy_for(
    class: CommandClass,
    impact: RuntimeImpact,
    peripheral: &ChangedOwnerInputs,
    intent: CoreRunIntent,
) -> CommandPolicy {
    // An explicit switch owes a target whatever its diff says. Re-selecting the
    // profile that is already current, or re-picking the running core, produces
    // no impact at all, and treating that as a plain save would drop the
    // confirmation the command promised: an empty diff is not evidence that the
    // target ever converged.
    let critical = class == CommandClass::ExplicitSwitch || impact != RuntimeImpact::None;

    if !critical {
        // Nothing critical to apply, so the only question left is whether any
        // peripheral owner was handed a target to reconcile after the commit.
        return if peripheral.is_empty() {
            CommandPolicy::SaveOnly
        } else {
            CommandPolicy::SaveThenNotify
        };
    }

    if intent == CoreRunIntent::StoppedByUser {
        return CommandPolicy::SavedInactive;
    }

    match (class, impact) {
        // Decided by the command: it named something that has to be running.
        (CommandClass::ExplicitSwitch, _) => CommandPolicy::MustApply,
        // Which binary or host the user's traffic runs through is not something
        // to defer: committing it while the old one keeps running would leave
        // the app claiming a core it never started.
        (_, RuntimeImpact::CoreSwap | RuntimeImpact::HostSwitch) => CommandPolicy::MustApply,
        // A `Save` with no impact returned above, so this one really moved a
        // build input.
        (CommandClass::Save, _) => CommandPolicy::AllowDeferredWhenSafe,
    }
}

/// How a critical Try ended, as typed by the adapter that could observe it.
///
/// `CoreErrorKind::Internal`, wait timeouts and lost receipts belong in
/// [`Unknown`], never in [`Transient`]: none of them says what the core did.
///
/// [`Unknown`]: TryCauseKind::Unknown
/// [`Transient`]: TryCauseKind::Transient
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TryCauseKind {
    /// The outcome could not be observed. Never enters the retryable branch.
    Unknown,
    /// A check or the core itself rejected this candidate. The same desired
    /// value cannot succeed later.
    Deterministic,
    /// Observed, finished, and permanent for now: a refused elevation, a
    /// missing binary, an unsupported platform.
    Permanent,
    /// Observed, finished, and typed transient by the adapter that watched it.
    Transient,
}

/// Whether the application knows what is running after the failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BaselineAvailability {
    /// A known-good runtime is still running, or the core is confirmed stopped.
    Known,
    /// The real state could not be confirmed. A backend that answers
    /// "rolled back" has described its own request, not the running config.
    Unconfirmed,
}

/// The typed facts a deferral decision may read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TryFailureFacts {
    pub cause: TryCauseKind,
    pub baseline: BaselineAvailability,
    pub policy: CommandPolicy,
    /// The candidate holds an item a deterministic check rejected, so no later
    /// attempt at it can converge.
    pub candidate_has_invalid_item: bool,
}

/// What the commit decision may be after a critical Try failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailureDisposition {
    /// Every condition holds: commit the new desired value and mark the target
    /// deferred. What is applied stays at the baseline.
    Deferrable,
    /// Do not commit. The baseline is known, so the mutation is simply refused.
    Reject,
    /// The real state is unknown, so no commit decision can be derived from it
    /// at all. The operation has to be resolved before anything else runs.
    RecoveryRequired,
}

/// The whole conjunction, in one place.
///
/// Deferring means committing a value the core is not running, so it takes all
/// the typed failure, safe baseline, command policy and valid candidate.
pub(crate) fn disposition(facts: &TryFailureFacts) -> FailureDisposition {
    // An unobserved outcome decides nothing: the candidate may already run.
    if facts.cause == TryCauseKind::Unknown {
        return FailureDisposition::RecoveryRequired;
    }
    if facts.baseline == BaselineAvailability::Unconfirmed {
        return FailureDisposition::RecoveryRequired;
    }

    let deferrable = facts.cause == TryCauseKind::Transient
        && facts.policy.allows_deferral()
        && !facts.candidate_has_invalid_item;

    if deferrable {
        FailureDisposition::Deferrable
    } else {
        FailureDisposition::Reject
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::effects::plan::{
        ApplicationEffectInputs, ApplicationEffectPlan, EffectKind,
    };
    use nyanpasu_config::{
        application::NyanpasuAppConfig, clash::config::ClashConfig,
        runtime::executor::ResolvedPortBindings,
    };

    /// Owners are a diff result, so they are built through one here too rather
    /// than hand-assembled.
    fn peripheral(mutate: impl FnOnce(&mut NyanpasuAppConfig)) -> ChangedOwnerInputs {
        let app = NyanpasuAppConfig {
            language: nyanpasu_config::application::I18nLanguage::English,
            tray_menu_mode: nyanpasu_config::application::TrayMenuMode::Native,
            ..NyanpasuAppConfig::default()
        };
        let clash = ClashConfig::default();
        let ports = Some(ResolvedPortBindings {
            mixed_port: 7890,
            ..ResolvedPortBindings::default()
        });
        let previous = ApplicationEffectInputs::project(&app, &clash, ports.clone());
        let mut candidate_app = app;
        mutate(&mut candidate_app);
        let candidate = ApplicationEffectInputs::project(&candidate_app, &clash, ports);
        ChangedOwnerInputs::diff(&previous, &candidate)
    }

    fn unchanged() -> ChangedOwnerInputs {
        peripheral(|_| {})
    }

    fn system_proxy_changed() -> ChangedOwnerInputs {
        peripheral(|app| app.enable_system_proxy = true)
    }

    #[test]
    fn an_owner_diff_is_what_separates_save_only_from_save_then_notify() {
        assert!(unchanged().is_empty());
        assert!(system_proxy_changed().contains(EffectKind::SystemProxy));

        assert_eq!(
            policy_for(
                CommandClass::Save,
                RuntimeImpact::None,
                &unchanged(),
                CoreRunIntent::Running
            ),
            CommandPolicy::SaveOnly
        );
        assert_eq!(
            policy_for(
                CommandClass::Save,
                RuntimeImpact::None,
                &system_proxy_changed(),
                CoreRunIntent::Running
            ),
            CommandPolicy::SaveThenNotify
        );
    }

    /// `RuntimeImpact::None` is in the list on purpose. Re-selecting the profile
    /// that is already current classifies as no impact, and so do `update_core`
    /// and `set_execution_host` called with the value already stored; all three
    /// still owe the confirmation the command promised.
    #[test]
    fn an_explicit_switch_must_apply() {
        for impact in [
            RuntimeImpact::None,
            RuntimeImpact::Reconcile,
            RuntimeImpact::CoreSwap,
            RuntimeImpact::HostSwitch,
        ] {
            for owners in [unchanged(), system_proxy_changed()] {
                assert_eq!(
                    policy_for(
                        CommandClass::ExplicitSwitch,
                        impact,
                        &owners,
                        CoreRunIntent::Running
                    ),
                    CommandPolicy::MustApply,
                    "{impact:?} with owners {:?}",
                    owners.kinds()
                );
            }
        }
    }

    /// A stopped core outranks the switch: the promise is still not kept by
    /// starting a core the user asked to stop.
    #[test]
    fn a_stopped_core_turns_even_a_no_op_explicit_switch_into_a_saved_inactive() {
        assert_eq!(
            policy_for(
                CommandClass::ExplicitSwitch,
                RuntimeImpact::None,
                &unchanged(),
                CoreRunIntent::StoppedByUser
            ),
            CommandPolicy::SavedInactive
        );
    }

    #[test]
    fn an_ordinary_runtime_save_may_defer() {
        assert_eq!(
            policy_for(
                CommandClass::Save,
                RuntimeImpact::Reconcile,
                &unchanged(),
                CoreRunIntent::Running
            ),
            CommandPolicy::AllowDeferredWhenSafe
        );
    }

    /// Which binary or host carries the user's traffic is never deferred, even
    /// when the change arrived inside an ordinary save.
    #[test]
    fn a_save_that_moves_the_core_or_the_host_must_still_apply() {
        for impact in [RuntimeImpact::CoreSwap, RuntimeImpact::HostSwitch] {
            assert_eq!(
                policy_for(
                    CommandClass::Save,
                    impact,
                    &unchanged(),
                    CoreRunIntent::Running
                ),
                CommandPolicy::MustApply,
                "{impact:?}"
            );
        }
    }

    #[test]
    fn a_stopped_core_turns_every_runtime_change_into_a_saved_inactive() {
        for class in [CommandClass::ExplicitSwitch, CommandClass::Save] {
            for impact in [
                RuntimeImpact::Reconcile,
                RuntimeImpact::CoreSwap,
                RuntimeImpact::HostSwitch,
            ] {
                assert_eq!(
                    policy_for(class, impact, &unchanged(), CoreRunIntent::StoppedByUser),
                    CommandPolicy::SavedInactive,
                    "{class:?} {impact:?}"
                );
            }
        }
    }

    /// A stopped core is not a reason to stop reconciling the system proxy or
    /// the tray: those owners never needed the core.
    #[test]
    fn a_stopped_core_still_notifies_peripheral_owners() {
        assert_eq!(
            policy_for(
                CommandClass::Save,
                RuntimeImpact::None,
                &system_proxy_changed(),
                CoreRunIntent::StoppedByUser
            ),
            CommandPolicy::SaveThenNotify
        );
    }

    #[test]
    fn only_allow_deferred_when_safe_permits_a_deferral() {
        for policy in [
            CommandPolicy::MustApply,
            CommandPolicy::SaveOnly,
            CommandPolicy::SaveThenNotify,
            CommandPolicy::SavedInactive,
        ] {
            assert!(!policy.allows_deferral(), "{policy:?}");
        }
        assert!(CommandPolicy::AllowDeferredWhenSafe.allows_deferral());
    }

    /// The one fact pattern where every condition of the conjunction holds.
    fn deferrable() -> TryFailureFacts {
        TryFailureFacts {
            cause: TryCauseKind::Transient,
            baseline: BaselineAvailability::Known,
            policy: CommandPolicy::AllowDeferredWhenSafe,
            candidate_has_invalid_item: false,
        }
    }

    #[test]
    fn every_condition_holding_is_what_makes_a_failure_deferrable() {
        assert_eq!(disposition(&deferrable()), FailureDisposition::Deferrable);
    }

    #[test]
    fn dropping_any_single_condition_stops_the_deferral() {
        struct Case {
            condition: &'static str,
            break_it: fn(&mut TryFailureFacts),
            expected: FailureDisposition,
        }

        const CASES: &[Case] = &[
            Case {
                condition: "the cause is not typed retryable",
                break_it: |facts| facts.cause = TryCauseKind::Permanent,
                expected: FailureDisposition::Reject,
            },
            Case {
                condition: "the candidate itself was rejected",
                break_it: |facts| facts.cause = TryCauseKind::Deterministic,
                expected: FailureDisposition::Reject,
            },
            Case {
                condition: "the outcome was not observed",
                break_it: |facts| facts.cause = TryCauseKind::Unknown,
                expected: FailureDisposition::RecoveryRequired,
            },
            Case {
                condition: "no safe baseline is known",
                break_it: |facts| facts.baseline = BaselineAvailability::Unconfirmed,
                expected: FailureDisposition::RecoveryRequired,
            },
            Case {
                condition: "the command does not allow deferring",
                break_it: |facts| facts.policy = CommandPolicy::MustApply,
                expected: FailureDisposition::Reject,
            },
            Case {
                condition: "the candidate holds a deterministically invalid item",
                break_it: |facts| facts.candidate_has_invalid_item = true,
                expected: FailureDisposition::Reject,
            },
        ];

        for case in CASES {
            let mut facts = deferrable();
            (case.break_it)(&mut facts);
            assert_eq!(
                disposition(&facts),
                case.expected,
                "broken condition: {}",
                case.condition
            );
        }
    }

    /// An unobserved outcome cannot be rejected either: the candidate may be
    /// running, so the state has to be established before anything else.
    #[test]
    fn an_unknown_outcome_never_enters_the_retryable_branch() {
        let facts = TryFailureFacts {
            cause: TryCauseKind::Unknown,
            ..deferrable()
        };

        assert_eq!(disposition(&facts), FailureDisposition::RecoveryRequired);
        assert_ne!(disposition(&facts), FailureDisposition::Deferrable);
    }

    /// A terminated, observed, transient failure with a lost baseline is still
    /// not rejectable: "rolled back" is a claim about a request, not about what
    /// the core is running.
    #[test]
    fn an_unconfirmed_baseline_requires_recovery_rather_than_a_rejection() {
        let facts = TryFailureFacts {
            baseline: BaselineAvailability::Unconfirmed,
            ..deferrable()
        };
        assert_eq!(disposition(&facts), FailureDisposition::RecoveryRequired);
    }

    /// `ApplicationEffectPlan` is reachable from here, so the import that gives
    /// the owner diff its meaning stays honest.
    #[test]
    fn an_empty_plan_produces_no_owner_inputs() {
        let app = NyanpasuAppConfig::default();
        let clash = ClashConfig::default();
        let inputs = ApplicationEffectInputs::project(&app, &clash, None);
        assert!(ApplicationEffectPlan::diff(&inputs, &inputs).is_empty());
        assert!(unchanged().is_empty());
    }
}
