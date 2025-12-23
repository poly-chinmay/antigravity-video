use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EditPlan {
    pub actions: Vec<EditAction>,
    pub thought_process: Option<String>,
    pub confidence: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EditAction {
    #[serde(rename = "type")]
    pub action_type: ActionType,
    pub target_clip_id: String,
    pub parameters: Option<ActionParameters>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActionType {
    Delete,
    Move,
    Trim,
    Split,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ActionParameters {
    pub new_start_time: Option<f64>,
    pub trim_start_delta: Option<f64>,
    pub trim_end_delta: Option<f64>,
    pub split_time: Option<f64>,
}

impl EditAction {
    pub fn is_delete(&self) -> bool {
        self.action_type == ActionType::Delete
    }

    pub fn is_split(&self) -> bool {
        self.action_type == ActionType::Split
    }
}
