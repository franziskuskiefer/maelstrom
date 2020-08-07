pub struct Group {
    pub config: GroupConfig,
    pub identity: Identity,
    pub group_context: GroupContext,
    pub generation: u32,
    pub epoch_secrets: EpochSecrets,
    pub astree: ASTree,
    pub tree: Tree,
    pub public_queue: ProposalQueue,
    pub own_queue: ProposalQueue,
    pub pending_kpbs: Vec<KeyPackageBundle>,
    pub interim_transcript_hash: Vec<u8>,
}

pub struct Client {
    key_packages: Vec<(KeyPakage, HPKEPrivateKey)>,
    identity: Identity,
}

trait GroupOps {
    // Create new group.
    fn new(creator: &Client, id: &[u8], ciphersuite: Ciphersuite, validator: Validator) -> Self;
    // Join a group from a welcome message
    fn new_from_welcome(joiner: &Client, welcome_msg: Welcome, tree: Tree, validator: Validator) -> Self;

    // Create handshake messages
    fn create_add_proposal(&self, aad: &[u8], joiner_key_package: KeyPackage) -> MLSPlaintext;
    fn create_update_proposal(&self, aad: &[u8]) -> MLSPlaintext;
    fn create_remove_proposal(&self, aad: &[u8], removed_index: LeafIndex) -> MLSPlaintext;
    fn create_commit(&self, aad: &[u8]) -> MLSPlaintext;
    
    // Process handshake messages
    fn process_commit(&self, msg: MLSPlaintext) -> MembershipChanges;
    fn process_proposal(&self, msg: MLSPlaintext) -> MembershipChanges;
    
    // Create application message
    fn create_application_message(&self, aad: &[u8], msg: &[u8]) -> MLSPlaintext;
    // Process application message
    fn process_application_message(&self, msg: MLSPlaintext) -> Vec<u8>;

    // Encrypt/Decrypt MLS message
    fn encrypt(ptxt: MLSPlaintext) -> MLSCiphertext;
    fn decrypt(ctxt: MLSCiphertext) -> MLSPlaintext;
}
