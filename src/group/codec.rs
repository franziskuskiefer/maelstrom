use crate::group::*;

impl Codec for Group {
    fn encode(&self, buffer: &mut Vec<u8>) -> Result<(), CodecError> {
        self.config.encode(buffer)?;
        self.identity.encode(buffer)?;
        self.group_context.encode(buffer)?;
        self.generation.encode(buffer)?;
        self.epoch_secrets.encode(buffer)?;
        self.astree.encode(buffer)?;
        self.tree.encode(buffer)?;
        self.public_queue.encode(buffer)?;
        self.own_queue.encode(buffer)?;
        encode_vec(VecSize::VecU32, buffer, &self.pending_kpbs)?;
        encode_vec(VecSize::VecU8, buffer, &self.interim_transcript_hash)?;
        Ok(())
    }
    fn decode(cursor: &mut Cursor) -> Result<Self, CodecError> {
        let config = GroupConfig::decode(cursor)?;
        let identity = Identity::decode(cursor)?;
        let group_context = GroupContext::decode(cursor)?;
        let generation = u32::decode(cursor)?;
        let epoch_secrets = EpochSecrets::decode(cursor)?;
        let astree = ASTree::decode(cursor)?;
        let tree = Tree::decode(cursor)?;
        let public_queue = ProposalQueue::decode(cursor)?;
        let own_queue = ProposalQueue::decode(cursor)?;
        let pending_kpbs = decode_vec(VecSize::VecU32, cursor)?;
        let interim_transcript_hash = decode_vec(VecSize::VecU8, cursor)?;
        let group = Group {
            config,
            identity,
            group_context,
            generation,
            epoch_secrets,
            astree,
            tree,
            public_queue,
            own_queue,
            pending_kpbs,
            interim_transcript_hash,
        };
        Ok(group)
    }
}
