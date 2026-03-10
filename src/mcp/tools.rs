use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

pub fn mesh_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "mesh_register".to_string(),
            description: "Register this agent with the AgentMesh broker. Call this at the start of a session to join the agent network.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Agent name (e.g., 'orunla-dev', 'website-agent')"
                    },
                    "project": {
                        "type": "string",
                        "description": "Project name this agent is working on (used for routing)"
                    },
                    "project_path": {
                        "type": "string",
                        "description": "Absolute path to the project directory"
                    },
                    "capabilities": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of capabilities: code_review, project_status, code_edit, testing, domain_expert"
                    }
                },
                "required": ["name", "project"]
            }),
        },
        ToolDefinition {
            name: "mesh_discover".to_string(),
            description: "Discover other agents on the AgentMesh network. Filter by project, capability, or platform.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project": {
                        "type": "string",
                        "description": "Filter by project name (optional)"
                    },
                    "capability": {
                        "type": "string",
                        "description": "Filter by capability (optional)"
                    },
                    "online_only": {
                        "type": "boolean",
                        "description": "Only show online agents (default: true)",
                        "default": true
                    }
                }
            }),
        },
        ToolDefinition {
            name: "mesh_ask".to_string(),
            description: "Ask a question to another agent or project. The broker routes to a live agent if available, or falls back to a proxy agent that reads project context and answers via LLM.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "to": {
                        "type": "string",
                        "description": "Target agent_id or project name"
                    },
                    "question": {
                        "type": "string",
                        "description": "The question to ask"
                    },
                    "data": {
                        "type": "object",
                        "description": "Optional structured data to include with the question"
                    }
                },
                "required": ["to", "question"]
            }),
        },
        ToolDefinition {
            name: "mesh_check_messages".to_string(),
            description: "Check for incoming messages in your inbox. Returns unread questions from other agents that you can respond to.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Maximum messages to return (default: 10)",
                        "default": 10
                    }
                }
            }),
        },
        ToolDefinition {
            name: "mesh_respond".to_string(),
            description: "Respond to an incoming message. Use after checking messages with mesh_check_messages.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message_id": {
                        "type": "string",
                        "description": "The ID of the message you are responding to"
                    },
                    "response": {
                        "type": "string",
                        "description": "Your response text"
                    }
                },
                "required": ["message_id", "response"]
            }),
        },
        ToolDefinition {
            name: "mesh_status".to_string(),
            description: "Get the current status of the AgentMesh broker: connected agents, pending messages, uptime.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
    ]
}
