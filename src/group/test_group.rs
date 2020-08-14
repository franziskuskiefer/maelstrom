#[test]
fn group_operations() {
    use crate::ciphersuite::*;
    use crate::creds::*;
    use crate::group::*;

    let ciphersuite =
        Ciphersuite::new(CiphersuiteName::MLS10_128_DHKEMX25519_AES128GCM_SHA256_Ed25519);

    // Define identities
    let alice_identity = Identity::new(ciphersuite, b"Alice");
    let bob_identity = Identity::new(ciphersuite, b"Bob");
    let charlie_identity = Identity::new(ciphersuite, b"Charlie");

    let _alice_credential = BasicCredential::from(&alice_identity);
    let _bob_credential = BasicCredential::from(&bob_identity);
    let _charlie_credential = BasicCredential::from(&bob_identity);

    // Generate KeyPackages
    let bob_key_package_bundle = KeyPackageBundle::new(ciphersuite, &bob_identity, None);
    let bob_key_package = bob_key_package_bundle.get_key_package();

    let charlie_key_package_bundle = KeyPackageBundle::new(ciphersuite, &charlie_identity, None);
    let charlie_key_package = charlie_key_package_bundle.get_key_package();

    // Create a group with Alice
    let config = GroupConfig::new(ciphersuite);

    let mut group_alice = Group::new(alice_identity, GroupId::random(), config);

    // Alice sends a message to herself
    let message_alice = [1, 2, 3];
    let mls_plaintext = group_alice.create_application_message(&message_alice, Some(&[4, 5, 6]));
    let encrypted_message = group_alice.encrypt(&mls_plaintext);
    let decrypted_mls_plaintext = group_alice.decrypt(&encrypted_message);
    assert_eq!(mls_plaintext, decrypted_mls_plaintext);

    // Alice adds Bob
    let _bob_add_proposal = group_alice.create_add_proposal(&bob_key_package, None);

    let (commit1, ms1, welcome_bundle_alice_bob) = group_alice.create_commit(None);

    group_alice.process_commit(commit1);

    let mut group_bob = Group::new_from_welcome(
        bob_identity,
        welcome_bundle_alice_bob.unwrap(),
        bob_key_package_bundle,
    );

    assert_eq!(group_alice.get_tree().nodes, group_bob.get_tree().nodes);
    group_alice.get_tree().print(&format!("\n{:?}", ms1));

    // Alice sends a message to Bob
    let message_alice = [1, 2, 3];
    let mls_plaintext_alice = group_alice.create_application_message(&message_alice, None);
    let encrypted_message = group_alice.encrypt(&mls_plaintext_alice);
    let mls_plaintext_bob = group_bob.decrypt(&encrypted_message);
    assert_eq!(mls_plaintext_alice, mls_plaintext_bob);

    // Bob updates and commits
    let update_proposal_bob = group_bob.create_update_proposal(None);
    let (commit2, ms2, _) = group_bob.create_commit(None);

    group_alice.process_proposal(update_proposal_bob);
    group_alice.process_commit(commit2.clone());
    group_bob.process_commit(commit2);

    group_alice.tree.print(&format!("\n{:?}", ms2));

    // Alice updates and commits
    let update_proposal_alice = group_alice.create_update_proposal(None);
    let (commit3, ms3, _) = group_alice.create_commit(None);

    group_bob.process_proposal(update_proposal_alice);
    group_alice.process_commit(commit3.clone());
    group_bob.process_commit(commit3);

    group_alice.tree.print(&format!("\n{:?}", ms3));

    // Alice updates and Bob commits
    let update_proposal_alice = group_alice.create_update_proposal(None);
    group_bob.process_proposal(update_proposal_alice);
    let (commit4, ms4, _) = group_bob.create_commit(None);

    group_bob.process_commit(commit4.clone());
    group_alice.process_commit(commit4);

    group_alice.tree.print(&format!("\n{:?}", ms4));

    // Bob updates and Alice commits
    let update_proposal_bob = group_bob.create_update_proposal(None);
    group_alice.process_proposal(update_proposal_bob);
    let (commit5, ms5, _) = group_alice.create_commit(None);

    group_alice.process_commit(commit5.clone());
    group_bob.process_commit(commit5);

    group_alice.tree.print(&format!("\n{:?}", ms5));

    // Bob adds Charlie
    let add_proposal = group_bob.create_add_proposal(&charlie_key_package, None);
    group_alice.process_proposal(add_proposal);

    let (commit6, ms6, welcome_bundle_bob_charlie) = group_bob.create_commit(None);

    group_alice.process_commit(commit6.clone());
    group_bob.process_commit(commit6);

    let mut group_charlie = Group::new_from_welcome(
        charlie_identity,
        welcome_bundle_bob_charlie.unwrap(),
        charlie_key_package_bundle,
    );

    group_alice.tree.print(&format!("\n{:?}", ms6));

    // Charlie updates
    let update_proposal_charlie = group_charlie.create_update_proposal(None);

    group_alice.process_proposal(update_proposal_charlie.clone());
    group_bob.process_proposal(update_proposal_charlie);

    let (commit7, ms7, _) = group_charlie.create_commit(None);

    group_alice.process_commit(commit7.clone());
    group_bob.process_commit(commit7.clone());
    group_charlie.process_commit(commit7);

    group_alice.tree.print(&format!("\n{:?}", ms7));

    // Alice updates
    let update_proposal_alice = group_alice.create_update_proposal(None);

    group_bob.process_proposal(update_proposal_alice.clone());
    group_charlie.process_proposal(update_proposal_alice);

    let (commit8, ms8, _) = group_alice.create_commit(None);

    group_alice.process_commit(commit8.clone());
    group_bob.process_commit(commit8.clone());
    group_charlie.process_commit(commit8);

    group_alice.tree.print(&format!("\n{:?}", ms8));

    // Charlie removes Bob
    let remove_proposal_charlie = group_charlie.create_remove_proposal(2, None);

    group_alice.process_proposal(remove_proposal_charlie.clone());
    group_bob.process_proposal(remove_proposal_charlie);

    let (commit9, ms9, _) = group_charlie.create_commit(None);

    group_alice.process_commit(commit9.clone());
    group_bob.process_commit(commit9.clone());
    group_charlie.process_commit(commit9);

    group_alice.tree.print(&format!("\n{:?}", ms9));
}

#[test]
fn print_group() {
    use crate::ciphersuite::*;
    use crate::creds::*;
    use crate::group::*;

    let ciphersuite =
        Ciphersuite::new(CiphersuiteName::MLS10_128_DHKEMX25519_AES128GCM_SHA256_Ed25519);

    // Define identities
    let alice_identity = Identity::new(ciphersuite, b"Alice");
    let bob_identity = Identity::new(ciphersuite, b"Bob");

    // Create a group with Alice
    let config = GroupConfig::new(ciphersuite);

    let mut group_alice = Group::new(alice_identity, GroupId::random(), config);
    group_alice.get_tree().print("Initial tree after setup.");

    // Generate KeyPackages
    let bob_key_package_bundle = KeyPackageBundle::new(ciphersuite, &bob_identity, None);
    let bob_key_package = bob_key_package_bundle.get_key_package();


    // Alice adds Bob
    let bob_add_proposal = group_alice.create_add_proposal(&bob_key_package, None);
    println!("Alice sends proposal: {:?}", bob_add_proposal);

    let (commit1, _ms1, welcome_bundle_alice_bob) = group_alice.create_commit(None);

    group_alice.process_commit(commit1);
    group_alice.get_tree().print("Group including Bob now.");

    let _group_bob = Group::new_from_welcome(
        bob_identity,
        welcome_bundle_alice_bob.unwrap(),
        bob_key_package_bundle,
    );
}
