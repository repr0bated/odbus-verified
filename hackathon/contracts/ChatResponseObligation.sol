// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// Public record of the `chat_response` plugin.
///
/// The chatbot closes a turn in exactly one of three ways, the same three
/// methods the plugin already requires:
///   respond_to_user, request_clarification, cannot_perform.
///
/// Words stay in the human's principal. This contract stores the hashes the
/// plugin already publishes (`chat_response_methods.rs`: genesis domain
/// `opdbus.chat_response.genesis.v1`, turn domain
/// `opdbus.chat_response.turn.v1`). It checks that each turn follows the
/// previous head. It does not recompute the canonical JSON hash and it does
/// not accept message text.
///
/// Not deployed. Intended public testnet, already named in hackathon/CONTEXT.md:
/// Base Sepolia, chain id 84532. No address until a key records it.
contract ChatResponseObligation {
    enum Act {
        Respond,
        Clarify,
        Refuse
    }

    /// Same order as `RefusalCategory` in chat_response.rs. 255 means the
    /// turn was not a refusal.
    uint8 public constant NO_REFUSAL = 255;
    uint8 public constant REFUSAL_NOT_SUPPORTED = 4;

    struct Conversation {
        bytes32 genesisHash;
        bytes32 headTurnHash;
        uint64 turnCount;
        address recorder;
    }

    struct Turn {
        bytes32 conversationId;
        uint64 turnIndex;
        bytes32 prevTurnHash;
        bytes32 turnHash;
        bytes32 contentHash;
        Act act;
        bytes32 responder;
        uint8 refusalCategory;
    }

    mapping(bytes32 => Conversation) public conversations;
    mapping(bytes32 => Turn) public turns;

    event ConversationOpened(
        bytes32 indexed conversationId,
        bytes32 genesisHash,
        address recorder
    );

    event TurnCommitted(
        bytes32 indexed conversationId,
        uint64 turnIndex,
        bytes32 prevTurnHash,
        bytes32 indexed turnHash,
        bytes32 contentHash,
        Act act,
        uint8 refusalCategory
    );

    function openConversation(bytes32 conversationId, bytes32 genesisHash) external {
        require(conversationId != bytes32(0), "conversation");
        require(genesisHash != bytes32(0), "genesis");
        require(conversations[conversationId].recorder == address(0), "open");
        conversations[conversationId] = Conversation({
            genesisHash: genesisHash,
            headTurnHash: genesisHash,
            turnCount: 0,
            recorder: msg.sender
        });
        emit ConversationOpened(conversationId, genesisHash, msg.sender);
    }

    /// `turnHash` and `contentHash` are the 32-byte digests the plugin already
    /// computed. `responder` is the responder principal id, hashed, not a name.
    function commitTurn(
        bytes32 conversationId,
        uint64 turnIndex,
        bytes32 prevTurnHash,
        bytes32 turnHash,
        bytes32 contentHash,
        Act act,
        bytes32 responder,
        uint8 refusalCategory
    ) external {
        Conversation storage conversation = conversations[conversationId];
        require(conversation.recorder != address(0), "unknown");
        require(conversation.recorder == msg.sender, "recorder");
        require(turnIndex == conversation.turnCount, "index");
        require(prevTurnHash == conversation.headTurnHash, "chain");
        require(turnHash != bytes32(0) && contentHash != bytes32(0), "hash");
        require(responder != bytes32(0), "responder");
        require(turns[turnHash].turnHash == bytes32(0), "replay");
        if (act == Act.Refuse) {
            require(refusalCategory <= REFUSAL_NOT_SUPPORTED, "category");
        } else {
            require(refusalCategory == NO_REFUSAL, "category");
        }

        conversation.headTurnHash = turnHash;
        conversation.turnCount = turnIndex + 1;
        turns[turnHash] = Turn({
            conversationId: conversationId,
            turnIndex: turnIndex,
            prevTurnHash: prevTurnHash,
            turnHash: turnHash,
            contentHash: contentHash,
            act: act,
            responder: responder,
            refusalCategory: refusalCategory
        });
        emit TurnCommitted(
            conversationId,
            turnIndex,
            prevTurnHash,
            turnHash,
            contentHash,
            act,
            refusalCategory
        );
    }
}
