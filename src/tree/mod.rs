// maelstrom
// Copyright (C) 2020 Raphael Robert
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see http://www.gnu.org/licenses/.

use rayon::prelude::*;

use crate::ciphersuite::*;
use crate::codec::*;
use crate::creds::*;
use crate::extensions::*;
use crate::key_packages::*;
use crate::messages::*;
use crate::schedule::*;

// Tree modules
pub(crate) mod astree;
pub(crate) mod codec;
pub(crate) mod treemath;

// Internal tree tests
mod test_astree;
mod test_treemath;

#[derive(PartialEq, Clone, Copy, Debug)]
#[repr(u8)]
pub enum NodeType {
    Leaf = 0,
    Parent = 1,
    Default = 255,
}

impl From<u8> for NodeType {
    fn from(value: u8) -> Self {
        match value {
            0 => NodeType::Leaf,
            1 => NodeType::Parent,
            _ => NodeType::Default,
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Node {
    pub node_type: NodeType,
    pub key_package: Option<KeyPackage>,
    pub node: Option<ParentNode>,
}

impl Node {
    pub fn new_leaf(kp_option: Option<KeyPackage>) -> Self {
        Node {
            node_type: NodeType::Leaf,
            key_package: kp_option,
            node: None,
        }
    }
    pub fn new_blank_parent_node() -> Self {
        Node {
            node_type: NodeType::Parent,
            key_package: None,
            node: None,
        }
    }
    pub fn get_public_hpke_key(&self) -> Option<HPKEPublicKey> {
        match self.node_type {
            NodeType::Leaf => {
                if let Some(ref kp) = self.key_package {
                    Some(kp.get_hpke_init_key().clone())
                } else {
                    None
                }
            }
            NodeType::Parent => {
                if let Some(ref parent_node) = self.node {
                    Some(parent_node.public_key.clone())
                } else {
                    None
                }
            }
            NodeType::Default => None,
        }
    }
    pub fn blank(&mut self) {
        self.key_package = None;
        self.node = None;
    }
    pub fn is_blank(&self) -> bool {
        self.key_package.is_none() && self.node.is_none()
    }
    pub fn hash(&self, ciphersuite: Ciphersuite) -> Option<Vec<u8>> {
        if let Some(parent_node) = self.node.clone() {
            let payload = parent_node.encode_detached().unwrap();
            let node_hash = ciphersuite.hash(&payload);
            Some(node_hash)
        } else {
            None
        }
    }
    pub fn parent_hash(&self) -> Option<Vec<u8>> {
        if self.is_blank() {
            return None;
        }
        match self.node_type {
            NodeType::Parent => {
                if let Some(node) = self.node.clone() {
                    Some(node.parent_hash)
                } else {
                    None
                }
            }
            NodeType::Leaf => {
                if let Some(key_package) = self.key_package.clone() {
                    if let Some(parent_hash_extension) =
                        key_package.get_extension(ExtensionType::ParentHash)
                    {
                        if let ExtensionPayload::ParentHash(parent_hash_extension) =
                            parent_hash_extension
                        {
                            Some(parent_hash_extension.parent_hash)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct ParentNode {
    public_key: HPKEPublicKey,
    unmerged_leaves: Vec<u32>,
    parent_hash: Vec<u8>,
}

// TODO improve the storage memory footprint
#[derive(Default, Debug, Clone)]
pub struct PathKeypairs {
    keypairs: Vec<Option<HPKEKeyPair>>,
}

impl PathKeypairs {
    pub fn new() -> Self {
        PathKeypairs { keypairs: vec![] }
    }
    pub fn add(&mut self, keypairs: Vec<HPKEKeyPair>, path: Vec<NodeIndex>) {
        fn extend_vec(tree_keypairs: &mut PathKeypairs, max_index: NodeIndex) {
            while tree_keypairs.keypairs.len() <= max_index.as_usize() {
                tree_keypairs.keypairs.push(None);
            }
        }
        assert_eq!(keypairs.len(), path.len()); // TODO return error
        for i in 0..path.len() {
            let index = path[i];
            extend_vec(self, index);
            self.keypairs[index.as_usize()] = Some(keypairs[i].clone());
        }
    }
    pub fn get(&self, index: NodeIndex) -> Option<HPKEKeyPair> {
        if index.as_usize() >= self.keypairs.len() {
            return None;
        }
        self.keypairs.get(index.as_usize()).unwrap().clone()
    }
}

#[derive(Debug, Clone)]
pub struct OwnLeaf {
    pub ciphersuite: Ciphersuite,
    pub kpb: KeyPackageBundle,
    pub leaf_index: NodeIndex,
    pub path_keypairs: PathKeypairs,
}

impl OwnLeaf {
    pub fn new(
        ciphersuite: Ciphersuite,
        kpb: KeyPackageBundle,
        leaf_index: NodeIndex,
        path_keypairs: PathKeypairs,
    ) -> Self {
        Self {
            ciphersuite,
            kpb,
            leaf_index,
            path_keypairs,
        }
    }
    pub fn generate_path_secrets(
        ciphersuite: Ciphersuite,
        start_secret: &[u8],
        n: usize,
    ) -> (Vec<Vec<u8>>, CommitSecret) {
        let hash_len = ciphersuite.hash_length();
        let leaf_node_secret = hkdf_expand_label(ciphersuite, start_secret, "path", &[], hash_len);
        let mut path_secrets = vec![leaf_node_secret];
        for i in 0..n - 1 {
            let path_secret =
                hkdf_expand_label(ciphersuite, &path_secrets[i], "path", &[], hash_len);
            path_secrets.push(path_secret);
        }
        let commit_secret = CommitSecret(hkdf_expand_label(
            ciphersuite,
            &path_secrets.last().unwrap(),
            "path",
            &[],
            hash_len,
        ));
        (path_secrets, commit_secret)
    }
    pub fn continue_path_secrets(
        ciphersuite: Ciphersuite,
        intermediate_secret: &[u8],
        n: usize,
    ) -> (Vec<Vec<u8>>, CommitSecret) {
        let hash_len = ciphersuite.hash_length();
        let mut path_secrets = vec![intermediate_secret.to_vec()];
        for i in 0..n - 1 {
            let path_secret =
                hkdf_expand_label(ciphersuite, &path_secrets[i], "path", &[], hash_len);
            path_secrets.push(path_secret);
        }
        let commit_secret = CommitSecret(hkdf_expand_label(
            ciphersuite,
            &path_secrets.last().unwrap(),
            "path",
            &[],
            hash_len,
        ));
        (path_secrets, commit_secret)
    }
    pub fn generate_path_keypairs(
        ciphersuite: Ciphersuite,
        path_secrets: Vec<Vec<u8>>,
    ) -> Vec<HPKEKeyPair> {
        let hash_len = ciphersuite.hash_length();
        let mut keypairs = vec![];
        for path_secret in path_secrets {
            let node_secret = hkdf_expand_label(ciphersuite, &path_secret, "node", &[], hash_len);
            let keypair = HPKEKeyPair::from_slice(&node_secret, ciphersuite);
            keypairs.push(keypair);
        }
        keypairs
    }
}

#[derive(Debug, Clone)]
pub struct Tree {
    ciphersuite: Ciphersuite,
    pub nodes: Vec<Node>,
    pub own_leaf: OwnLeaf,
}

impl Tree {
    pub fn new(ciphersuite: Ciphersuite, kpb: KeyPackageBundle) -> Tree {
        let own_leaf = OwnLeaf::new(
            ciphersuite,
            kpb.clone(),
            NodeIndex::from(0u32),
            PathKeypairs::new(),
        );
        let nodes = vec![Node {
            node_type: NodeType::Leaf,
            key_package: Some(kpb.get_key_package().clone()),
            node: None,
        }];
        Tree {
            ciphersuite,
            nodes,
            own_leaf,
        }
    }
    pub fn new_from_nodes(
        ciphersuite: Ciphersuite,
        kpb: KeyPackageBundle,
        node_options: &[Option<Node>],
        index: NodeIndex,
    ) -> Tree {
        let mut nodes = Vec::with_capacity(node_options.len());
        for (i, node_option) in node_options.iter().enumerate() {
            if let Some(node) = node_option.clone() {
                nodes.push(node);
            } else if i % 2 == 0 {
                nodes.push(Node::new_leaf(None));
            } else {
                nodes.push(Node::new_blank_parent_node());
            }
        }
        let secret = kpb.get_private_key().as_slice();
        let dirpath = treemath::dirpath_root(index, LeafIndex::from(NodeIndex::from(nodes.len())));
        let (path_secrets, _commit_secret) =
            OwnLeaf::generate_path_secrets(ciphersuite, secret, dirpath.len());
        let keypairs = OwnLeaf::generate_path_keypairs(ciphersuite, path_secrets);
        let mut path_keypairs = PathKeypairs::new();
        path_keypairs.add(keypairs, dirpath);
        let own_leaf = OwnLeaf::new(ciphersuite, kpb, index, path_keypairs);
        Tree {
            ciphersuite,
            nodes,
            own_leaf,
        }
    }
    pub fn tree_size(&self) -> NodeIndex {
        NodeIndex::from(self.nodes.len())
    }

    pub fn public_key_tree(&self) -> Vec<Option<Node>> {
        let mut tree = vec![];
        for node in self.nodes.iter() {
            if node.is_blank() {
                tree.push(None)
            } else {
                tree.push(Some(node.clone()))
            }
        }
        tree
    }

    pub fn leaf_count(&self) -> LeafIndex {
        LeafIndex::from(self.tree_size())
    }

    fn resolve(&self, index: NodeIndex) -> Vec<NodeIndex> {
        let size = self.leaf_count();

        if self.nodes[index.as_usize()].node_type == NodeType::Leaf {
            if self.nodes[index.as_usize()].is_blank() {
                return vec![];
            } else {
                return vec![index];
            }
        }

        if !self.nodes[index.as_usize()].is_blank() {
            let mut nodes = vec![index];
            nodes.extend(
                self.nodes[index.as_usize()]
                    .clone()
                    .node
                    .unwrap()
                    .unmerged_leaves
                    .iter()
                    .map(|n| NodeIndex::from(*n)),
            );
            return nodes;
        }

        let mut left = self.resolve(treemath::left(index));
        let right = self.resolve(treemath::right(index, size));
        left.extend(right);
        left
    }
    pub fn blank_member(&mut self, index: NodeIndex) {
        let size = self.leaf_count();
        self.nodes[index.as_usize()].blank();
        self.nodes[treemath::root(size).as_usize()].blank();
        for index in treemath::dirpath(index, size) {
            self.nodes[index.as_usize()].blank();
        }
    }
    pub fn free_leaves(&self) -> Vec<NodeIndex> {
        let mut free_leaves = vec![];
        for i in 0..self.leaf_count().as_usize() {
            // TODO use an iterator instead
            if self.nodes[NodeIndex::from(LeafIndex::from(i)).as_usize()].is_blank() {
                free_leaves.push(NodeIndex::from(i));
            }
        }
        free_leaves
    }
    
    #[cfg(test)]
    pub fn print(&self, message: &str) {
        use crate::utils::*;
        let factor = 3;
        println!("{}", message);
        for (i, node) in self.nodes.iter().enumerate() {
            let level = treemath::level(NodeIndex::from(i));
            print!("{:04}", i);
            if !node.is_blank() {
                let key_bytes;
                let parent_hash_bytes: Vec<u8>;
                match node.node_type {
                    NodeType::Leaf => {
                        print!("\tL");
                        key_bytes = if let Some(kp) = &node.key_package {
                            kp.get_hpke_init_key().as_slice()
                        } else {
                            &[]
                        };
                        parent_hash_bytes = if let Some(kp) = node.key_package.clone() {
                            if let Some(phe) = kp.get_extension(ExtensionType::ParentHash) {
                                if let ExtensionPayload::ParentHash(parent_hash_inner) = phe {
                                    parent_hash_inner.parent_hash
                                } else {
                                    panic!("Wrong extension type: expected ParentHashExtension")
                                }
                            } else {
                                vec![]
                            }
                        } else {
                            vec![]
                        }
                    }
                    NodeType::Parent => {
                        print!("\tP");
                        key_bytes = if let Some(n) = &node.node {
                            n.public_key.as_slice()
                        } else {
                            &[]
                        };
                        parent_hash_bytes = if let Some(ph) = node.parent_hash() {
                            ph
                        } else {
                            vec![]
                        }
                    }
                    _ => unreachable!(),
                }
                if !key_bytes.is_empty() {
                    print!("\tPK: {}", bytes_to_hex(&key_bytes));
                } else {
                    print!("\tPK:\t\t\t");
                }

                if !parent_hash_bytes.is_empty() {
                    print!("\tPH: {}", bytes_to_hex(&parent_hash_bytes));
                } else {
                    print!("\tPH:\t\t\t\t\t\t\t\t");
                }
                print!("\t| ");
                for _ in 0..level * factor {
                    print!(" ");
                }
                print!("◼︎");
            } else {
                print!("\tB\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t| ");
                for _ in 0..level * factor {
                    print!(" ");
                }
                print!("❑");
            }
            println!();
        }
    }
    pub fn update_direct_path(
        &mut self,
        sender: LeafIndex,
        direct_path: DirectPath,
        key_package: KeyPackage,
        group_context: &[u8],
    ) -> CommitSecret {
        let own_index = self.own_leaf.leaf_index;
        // TODO check that the direct path is long enough
        let common_ancestor =
            treemath::common_ancestor(NodeIndex::from(sender), self.own_leaf.leaf_index);
        let sender_dirpath = treemath::dirpath_root(NodeIndex::from(sender), self.leaf_count());
        let sender_copath = treemath::copath(NodeIndex::from(sender), self.leaf_count());
        let common_ancestor_sender_dirpath_index = sender_dirpath
            .iter()
            .position(|x| *x == common_ancestor)
            .unwrap();
        let common_ancestor_copath_index = sender_copath[common_ancestor_sender_dirpath_index];
        let resolution = self.resolve(common_ancestor_copath_index);
        // TODO Check if own node is in resolution (should always be the case)
        let position_in_resolution = resolution
            .iter()
            .position(|x| *x == self.own_leaf.leaf_index)
            .unwrap_or(0);
        // TODO Check resolution.len() == encrypted_path_secret.len()
        let hpke_ciphertext = direct_path.nodes[common_ancestor_sender_dirpath_index]
            .encrypted_path_secret[position_in_resolution]
            .clone();

        let private_key = if resolution[position_in_resolution] == own_index {
            self.own_leaf.kpb.get_private_key().clone()
        } else {
            match self
                .own_leaf
                .path_keypairs
                .get(common_ancestor_copath_index)
            {
                // FIXME: don't clone
                Some(key_pair) => key_pair.get_private_key().clone(),
                None => panic!("TODO: handle this."),
            }
        };
        let common_path = treemath::dirpath_long(common_ancestor, self.leaf_count());
        let secret = self
            .ciphersuite
            .hpke_open(hpke_ciphertext, &private_key, group_context, &[]);
        let (path_secrets, commit_secret) =
            OwnLeaf::continue_path_secrets(self.own_leaf.ciphersuite, &secret, common_path.len());
        let keypairs = OwnLeaf::generate_path_keypairs(self.own_leaf.ciphersuite, path_secrets);
        let sender_path_offset = sender_dirpath.len() - common_path.len();
        for (i, keypair) in keypairs.iter().enumerate().take(common_path.len()) {
            // TODO return an error if public keys don't match
            assert_eq!(
                &direct_path.nodes[sender_path_offset + i].public_key,
                keypair.get_public_key()
            );
        }
        self.merge_public_keys(direct_path, sender_dirpath);
        self.own_leaf
            .path_keypairs
            .add(keypairs.clone(), common_path.clone());
        self.merge_keypairs(keypairs, common_path);
        self.nodes[NodeIndex::from(sender).as_usize()] = Node::new_leaf(Some(key_package));
        self.compute_parent_hash(NodeIndex::from(sender));
        commit_secret
    }
    pub fn update_own_leaf(
        &mut self,
        identity: &Identity,
        key_pair: Option<&HPKEKeyPair>,
        kpb: Option<KeyPackageBundle>,
        group_context: &[u8],
        with_direct_path: bool,
    ) -> (
        CommitSecret,
        KeyPackageBundle,
        Option<DirectPath>,
        Option<Vec<Vec<u8>>>,
    ) {
        if key_pair.is_none() && kpb.is_none() {
            // TODO: Error handling.
            panic!("This must not happen.");
        }

        let own_index = self.own_leaf.leaf_index;
        let private_key = match key_pair {
            // FIXME: don't clone
            Some(k) => k.get_private_key().clone(),
            None => {
                debug_assert!(kpb.is_some());
                kpb.clone().unwrap().get_private_key().clone()
            }
        };
        let dirpath_root = treemath::dirpath_root(own_index, self.leaf_count());
        let node_secret = private_key.as_slice();
        let (path_secrets, confirmation) =
            OwnLeaf::generate_path_secrets(self.ciphersuite, &node_secret, dirpath_root.len());
        let keypairs = OwnLeaf::generate_path_keypairs(self.ciphersuite, path_secrets.clone());

        self.merge_keypairs(keypairs.clone(), dirpath_root.clone());

        let parent_hash = self.compute_parent_hash(own_index);
        let kpb = match kpb {
            Some(k) => k,
            None => {
                debug_assert!(key_pair.is_some());
                let parent_hash_extension = ParentHashExtension::new(&parent_hash);
                KeyPackageBundle::new_with_keypair(
                    self.ciphersuite,
                    identity,
                    Some(vec![parent_hash_extension.to_extension()]),
                    key_pair.unwrap(),
                )
            }
        };

        self.nodes[own_index.as_usize()] = Node::new_leaf(Some(kpb.get_key_package().clone()));
        let mut path_keypairs = PathKeypairs::new();
        path_keypairs.add(keypairs.clone(), dirpath_root);
        let own_leaf = OwnLeaf::new(self.ciphersuite, kpb.clone(), own_index, path_keypairs);
        self.own_leaf = own_leaf;
        if with_direct_path {
            (
                confirmation,
                kpb.clone(),
                Some(self.encrypt_to_copath(
                    path_secrets.clone(),
                    keypairs,
                    group_context,
                    kpb.get_key_package().clone(),
                )),
                Some(path_secrets),
            )
        } else {
            (confirmation, kpb, None, None)
        }
    }
    pub fn encrypt_to_copath(
        &self,
        path_secrets: Vec<Vec<u8>>,
        keypairs: Vec<HPKEKeyPair>,
        group_context: &[u8],
        leaf_key_package: KeyPackage,
    ) -> DirectPath {
        let copath = treemath::copath(self.own_leaf.leaf_index, self.leaf_count());
        assert_eq!(path_secrets.len(), copath.len()); // TODO return error
        assert_eq!(keypairs.len(), copath.len());
        let mut direct_path_nodes = vec![];
        let mut ciphertexts = vec![];
        for pair in path_secrets.iter().zip(copath.iter()) {
            let (path_secret, copath_node) = pair;
            let node_ciphertexts: Vec<HpkeCiphertext> = self
                .resolve(*copath_node)
                .par_iter()
                .map(|&x| {
                    let pk = self.nodes[x.as_usize()].get_public_hpke_key().unwrap();
                    self.ciphersuite
                        .hpke_seal(&pk, group_context, &[], &path_secret)
                })
                .collect();
            // TODO Check that all public keys are non-empty
            // TODO Handle potential errors
            ciphertexts.push(node_ciphertexts);
        }
        for pair in keypairs.iter().zip(ciphertexts.iter()) {
            let (keypair, node_ciphertexts) = pair;
            direct_path_nodes.push(DirectPathNode {
                public_key: keypair.get_public_key().clone(),
                encrypted_path_secret: node_ciphertexts.clone(),
            });
        }
        DirectPath {
            leaf_key_package,
            nodes: direct_path_nodes,
        }
    }
    pub fn merge_public_keys(&mut self, direct_path: DirectPath, path: Vec<NodeIndex>) {
        assert_eq!(direct_path.nodes.len(), path.len()); // TODO return error
        for (i, p) in path.iter().enumerate() {
            let public_key = direct_path.nodes[i].clone().public_key;
            let node = ParentNode {
                public_key: public_key.clone(),
                unmerged_leaves: vec![],
                parent_hash: vec![],
            };
            self.nodes[p.as_usize()].node = Some(node);
        }
    }
    pub fn merge_keypairs(&mut self, keypairs: Vec<HPKEKeyPair>, path: Vec<NodeIndex>) {
        assert_eq!(keypairs.len(), path.len()); // TODO return error
        for i in 0..path.len() {
            let node = ParentNode {
                public_key: keypairs[i].get_public_key().clone(),
                unmerged_leaves: vec![],
                parent_hash: vec![],
            };
            self.nodes[path[i].as_usize()].node = Some(node);
        }
    }
    pub fn apply_proposals(
        &mut self,
        proposal_id_list: ProposalIDList,
        proposal_queue: ProposalQueue,
        pending_kpbs: Vec<KeyPackageBundle>,
    ) -> (MembershipChanges, Vec<(NodeIndex, AddProposal)>, bool) {
        let mut updated_members = vec![];
        let mut removed_members = vec![];
        let mut added_members = Vec::with_capacity(proposal_id_list.adds.len());
        let mut invited_members = Vec::with_capacity(proposal_id_list.adds.len());

        let mut self_removed = false;

        for u in proposal_id_list.updates.iter() {
            let (_proposal_id, queued_proposal) = proposal_queue.get(&u).unwrap();
            let proposal = queued_proposal.proposal.clone();
            let update_proposal = proposal.as_update().unwrap();
            let sender = queued_proposal.sender;
            let index = sender.as_tree_index();
            let leaf_node = Node::new_leaf(Some(update_proposal.key_package.clone()));
            updated_members.push(update_proposal.key_package.get_credential().clone());
            self.blank_member(index);
            self.nodes[index.as_usize()] = leaf_node;
            if index == self.own_leaf.leaf_index {
                let own_kpb = pending_kpbs
                    .iter()
                    .find(|&kpb| kpb.get_key_package() == &update_proposal.key_package)
                    .unwrap();
                self.own_leaf = OwnLeaf::new(
                    self.ciphersuite,
                    own_kpb.clone(),
                    index,
                    PathKeypairs::new(),
                );
            }
        }
        for r in proposal_id_list.removes.iter() {
            let (_proposal_id, queued_proposal) = proposal_queue.get(&r).unwrap();
            let proposal = queued_proposal.proposal.clone();
            let remove_proposal = proposal.as_remove().unwrap();
            let removed = NodeIndex::from(remove_proposal.removed);
            if removed == self.own_leaf.leaf_index {
                self_removed = true;
            }
            let removed_member_node = self.nodes[removed.as_usize()].clone();
            let removed_member = if let Some(key_package) = removed_member_node.key_package {
                key_package
            } else {
                // TODO check it's really a leaf node
                panic!("Cannot remove a parent/empty node")
            };
            removed_members.push(removed_member.get_credential().clone());
            self.blank_member(removed);
        }

        if !proposal_id_list.adds.is_empty() {
            if proposal_id_list.adds.len() > (2 * self.leaf_count().as_usize()) {
                self.nodes.reserve_exact(
                    (2 * proposal_id_list.adds.len()) - (2 * self.leaf_count().as_usize()),
                );
            }
            let add_proposals: Vec<AddProposal> = proposal_id_list
                .adds
                .par_iter()
                .map(|a| {
                    let (_proposal_id, queued_proposal) = proposal_queue.get(&a).unwrap();
                    let proposal = queued_proposal.proposal.clone();
                    proposal.as_add().unwrap()
                })
                .collect();

            let free_leaves = self.free_leaves();
            // TODO make sure intermediary nodes are updated with unmerged_leaves
            let (add_in_place, add_append) = add_proposals.split_at(free_leaves.len());
            for (add_proposal, leaf_index) in add_in_place.iter().zip(free_leaves) {
                self.nodes[leaf_index.as_usize()] =
                    Node::new_leaf(Some(add_proposal.key_package.clone()));
                let dirpath = treemath::dirpath_root(leaf_index, self.leaf_count());
                for d in dirpath.iter() {
                    if !self.nodes[d.as_usize()].is_blank() {
                        let node = self.nodes[d.as_usize()].clone();
                        let index = d.as_u32();
                        // TODO handle error
                        let mut parent_node = node.node.unwrap();
                        if !parent_node.unmerged_leaves.contains(&index) {
                            parent_node.unmerged_leaves.push(index);
                        }
                        self.nodes[d.as_usize()].node = Some(parent_node);
                    }
                }
                added_members.push(add_proposal.key_package.get_credential().clone());
                invited_members.push((leaf_index, add_proposal.clone()));
            }
            let mut new_nodes = Vec::with_capacity(proposal_id_list.adds.len() * 2);
            let mut leaf_index = self.nodes.len() + 1;
            for add_proposal in add_append.iter() {
                new_nodes.extend(vec![
                    Node::new_blank_parent_node(),
                    Node::new_leaf(Some(add_proposal.key_package.clone())),
                ]);
                added_members.push(add_proposal.key_package.get_credential().clone());
                invited_members.push((NodeIndex::from(leaf_index), add_proposal.clone()));
                leaf_index += 2;
            }
            self.nodes.extend(new_nodes);
            self.trim_tree();
        }
        (
            MembershipChanges {
                updates: updated_members,
                removes: removed_members,
                adds: added_members,
            },
            invited_members,
            self_removed,
        )
    }
    pub fn trim_tree(&mut self) {
        let mut new_tree_size = 0;

        for i in 0..self.nodes.len() {
            if !self.nodes[i].is_blank() {
                new_tree_size = i + 1;
            }
        }

        if new_tree_size > 0 {
            self.nodes.truncate(new_tree_size);
        }
    }
    pub fn compute_tree_hash(&self) -> Vec<u8> {
        fn node_hash(ciphersuite: Ciphersuite, tree: &Tree, index: NodeIndex) -> Vec<u8> {
            let node: Node = tree.nodes[index.as_usize()].clone();
            match node.node_type {
                NodeType::Leaf => {
                    let leaf_node_hash = LeafNodeHashInput::new(index, node.key_package);
                    leaf_node_hash.hash(ciphersuite)
                }
                NodeType::Parent => {
                    let left = treemath::left(index);
                    let left_hash = node_hash(ciphersuite, tree, left);
                    let right = treemath::right(index, tree.leaf_count());
                    let right_hash = node_hash(ciphersuite, tree, right);
                    let parent_node_hash =
                        ParentNodeHashInput::new(index.as_u32(), node.node, left_hash, right_hash);
                    parent_node_hash.hash(ciphersuite)
                }
                NodeType::Default => panic!("Default node type not supported in tree hash."),
            }
        }
        let root = treemath::root(self.leaf_count());
        node_hash(self.ciphersuite, &self, root)
    }
    pub fn compute_parent_hash(&mut self, index: NodeIndex) -> Vec<u8> {
        let parent = treemath::parent(index, self.leaf_count());
        let parent_hash = if parent == treemath::root(self.leaf_count()) {
            let root_node = self.nodes[parent.as_usize()].clone();
            root_node.hash(self.own_leaf.ciphersuite).unwrap()
        } else {
            self.compute_parent_hash(parent)
        };
        let current_node = self.nodes[index.as_usize()].clone();
        if let Some(mut parent_node) = current_node.node {
            parent_node.parent_hash = parent_hash;
            self.nodes[index.as_usize()].node = Some(parent_node);
            let updated_parent_node = self.nodes[index.as_usize()].clone();
            updated_parent_node.hash(self.own_leaf.ciphersuite).unwrap()
        } else {
            parent_hash
        }
    }
    pub fn verify_integrity(ciphersuite: Ciphersuite, nodes: &[Option<Node>]) -> bool {
        let node_count = NodeIndex::from(nodes.len());
        let size = LeafIndex::from(node_count);
        for i in 0..node_count.as_usize() {
            let node_option = nodes[i].clone();
            if let Some(node) = node_option {
                match node.node_type {
                    NodeType::Parent => {
                        let left_index = treemath::left(NodeIndex::from(i));
                        let right_index = treemath::right(NodeIndex::from(i), size);
                        if right_index >= node_count {
                            return false;
                        }
                        let left_option = nodes[left_index.as_usize()].clone();
                        let right_option = nodes[right_index.as_usize()].clone();
                        let own_hash = node.hash(ciphersuite).unwrap();
                        if let Some(right) = right_option {
                            if let Some(left) = left_option {
                                let left_parent_hash = left.parent_hash().unwrap_or_else(Vec::new);
                                let right_parent_hash =
                                    right.parent_hash().unwrap_or_else(Vec::new);
                                if (left_parent_hash != own_hash) && (right_parent_hash != own_hash)
                                {
                                    return false;
                                }
                                if left_parent_hash == right_parent_hash {
                                    return false;
                                }
                            } else if right.parent_hash().unwrap() != own_hash {
                                return false;
                            }
                        } else if let Some(left) = left_option {
                            if left.parent_hash().unwrap() != own_hash {
                                return false;
                            }
                        }
                    }
                    NodeType::Leaf => {
                        if let Some(kp) = node.key_package {
                            if i % 2 != 0 {
                                return false;
                            }
                            if !kp.verify() {
                                return false;
                            }
                        }
                    }

                    NodeType::Default => {}
                }
            }
        }
        true
    }
}

pub struct ParentNodeHashInput {
    node_index: u32,
    parent_node: Option<ParentNode>,
    left_hash: Vec<u8>,
    right_hash: Vec<u8>,
}

impl ParentNodeHashInput {
    pub fn new(
        node_index: u32,
        parent_node: Option<ParentNode>,
        left_hash: Vec<u8>,
        right_hash: Vec<u8>,
    ) -> Self {
        Self {
            node_index,
            parent_node,
            left_hash,
            right_hash,
        }
    }
    pub fn hash(&self, ciphersuite: Ciphersuite) -> Vec<u8> {
        let payload = self.encode_detached().unwrap();
        ciphersuite.hash(&payload)
    }
}

pub struct LeafNodeHashInput {
    node_index: NodeIndex,
    key_package: Option<KeyPackage>,
}

impl LeafNodeHashInput {
    pub fn new(node_index: NodeIndex, key_package: Option<KeyPackage>) -> Self {
        Self {
            node_index,
            key_package,
        }
    }
    pub fn hash(&self, ciphersuite: Ciphersuite) -> Vec<u8> {
        let payload = self.encode_detached().unwrap();
        ciphersuite.hash(&payload)
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct DirectPathNode {
    pub public_key: HPKEPublicKey,
    pub encrypted_path_secret: Vec<HpkeCiphertext>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct DirectPath {
    pub leaf_key_package: KeyPackage,
    pub nodes: Vec<DirectPathNode>,
}
