use proptest_state_machine::{ReferenceStateMachine, StateMachineTest, prop_state_machine};
use uuid::Uuid;

use super::{Handle, KivalStateMachine, Model, OperationError, ResourceKind, ResourceMap};

/// Concrete mirror used to prove the reference machine integrates with Proptest.
struct MirrorTest;

impl StateMachineTest for MirrorTest {
    type Reference = KivalStateMachine;
    type SystemUnderTest = Model;

    fn init_test(
        reference: &<Self::Reference as ReferenceStateMachine>::State,
    ) -> Self::SystemUnderTest {
        reference.clone()
    }

    fn apply(
        state: Self::SystemUnderTest,
        _reference: &<Self::Reference as ReferenceStateMachine>::State,
        transition: <Self::Reference as ReferenceStateMachine>::Transition,
    ) -> Self::SystemUnderTest {
        KivalStateMachine::apply(state, &transition)
    }

    fn check_invariants(
        state: &Self::SystemUnderTest,
        reference: &<Self::Reference as ReferenceStateMachine>::State,
    ) {
        assert_eq!(state.workspace_count(), reference.workspace_count());
        assert_eq!(state.object_count(), reference.object_count());
    }
}

prop_state_machine! {
    #![proptest_config(proptest::test_runner::Config::with_cases(64))]
    #[test]
    fn generated_operations_remain_valid_after_shrinking(sequential 1..40 => MirrorTest);
}

#[test]
fn symbolic_handles_resolve_real_ids() {
    let mut resources = ResourceMap::default();
    let handle = Handle::new(ResourceKind::Workspace, 0);
    let id = Uuid::now_v7();

    resources.bind(handle, id).unwrap();

    assert_eq!(resources.resolve(handle).unwrap(), id);
}

#[test]
fn duplicate_symbolic_handle_bind_preserves_original_id() {
    let mut resources = ResourceMap::default();
    let handle = Handle::new(ResourceKind::Workspace, 0);
    let original_id = Uuid::from_u128(1);
    let replacement_id = Uuid::from_u128(2);

    resources.bind(handle, original_id).unwrap();

    assert!(matches!(
        resources.bind(handle, replacement_id),
        Err(OperationError::HandleAlreadyBound(bound)) if bound == handle
    ));
    assert_eq!(resources.resolve(handle).unwrap(), original_id);
}
