use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The node ownership of one persisted request record.
///
/// The values come from authenticated node identities only: the local node id
/// and, on a peer link, the remote node id proven by the handshake. A client
/// header or an optional peer-supplied field is never a source for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeAttribution {
    pub group_id: Uuid,
    pub ingress_node_id: Uuid,
    pub processing_node_id: Uuid,
}

impl NodeAttribution {
    /// A request that entered this node directly: the node is its own group,
    /// its ingress and its processor.
    pub fn local(node_id: Uuid) -> Self {
        Self {
            group_id: node_id,
            ingress_node_id: node_id,
            processing_node_id: node_id,
        }
    }

    /// A request that entered an edge node and was processed by its database
    /// node: the group and the processor are the database node, the ingress is
    /// the authenticated remote node of the link.
    pub fn peer_link(database_node_id: Uuid, remote_node_id: Uuid) -> Self {
        Self {
            group_id: database_node_id,
            ingress_node_id: remote_node_id,
            processing_node_id: database_node_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::NodeAttribution;
    use uuid::Uuid;

    #[test]
    fn a_local_request_belongs_to_the_node_it_entered() {
        let node_id = Uuid::from_u128(7);
        let attribution = NodeAttribution::local(node_id);

        assert_eq!(attribution.group_id, node_id);
        assert_eq!(attribution.ingress_node_id, node_id);
        assert_eq!(attribution.processing_node_id, node_id);
    }

    #[test]
    fn a_peer_request_names_the_authenticated_edge_as_its_ingress() {
        let database = Uuid::from_u128(7);
        let edge = Uuid::from_u128(11);
        let attribution = NodeAttribution::peer_link(database, edge);

        assert_eq!(attribution.group_id, database);
        assert_eq!(attribution.ingress_node_id, edge);
        assert_eq!(attribution.processing_node_id, database);
    }

    #[test]
    fn the_stored_document_holds_exactly_the_three_identities() {
        let database = Uuid::from_u128(7).to_string();
        let edge = Uuid::from_u128(11).to_string();
        let attribution = NodeAttribution::peer_link(Uuid::from_u128(7), Uuid::from_u128(11));
        let document = serde_json::to_value(attribution).expect("the attribution serializes");
        let object = document
            .as_object()
            .expect("the attribution is a JSON object");

        assert_eq!(object.len(), 3);
        assert_eq!(
            object.get("group_id").and_then(serde_json::Value::as_str),
            Some(database.as_str())
        );
        assert_eq!(
            object
                .get("ingress_node_id")
                .and_then(serde_json::Value::as_str),
            Some(edge.as_str())
        );
        assert_eq!(
            object
                .get("processing_node_id")
                .and_then(serde_json::Value::as_str),
            Some(database.as_str())
        );
    }
}
