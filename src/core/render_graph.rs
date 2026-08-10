use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum NodeType {
    Input,
    Transform,
    ColorCorrection,
    LUT,
    Blur,
    Sharpen,
    Keying,
    Mask,
    Blend,
    Crop,
    Speed,
    Stabilizer,
    NoiseReduction,
    Output,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub node_type: NodeType,
    pub label: String,
    pub inputs: Vec<NodeInput>,
    pub outputs: Vec<NodeOutput>,
    pub parameters: Vec<Parameter>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInput {
    pub name: String,
    pub connected_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeOutput {
    pub name: String,
    pub connected_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub value: ParameterValue,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub keyframes: Vec<Keyframe>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParameterValue {
    Float(f64),
    Int(i32),
    Bool(bool),
    Color(f32, f32, f32, f32),
    Point(f64, f64),
    String(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keyframe {
    pub time: f64,
    pub value: ParameterValue,
    pub interpolation: Interpolation,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Interpolation {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    Hold,
    Bezier(f64, f64, f64, f64),
}

#[derive(Debug)]
pub struct RenderGraph {
    nodes: Vec<Node>,
    next_id: usize,
}

impl RenderGraph {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            next_id: 0,
        }
    }

    pub fn add_node(&mut self, node_type: NodeType, label: &str) -> String {
        let id = format!("node_{}", self.next_id);
        self.next_id += 1;

        let (inputs, outputs) = Self::default_ports(node_type);
        let node = Node {
            id: id.clone(),
            node_type,
            label: label.to_string(),
            inputs,
            outputs,
            parameters: Self::default_parameters(&node_type),
            enabled: true,
        };

        self.nodes.push(node);
        id
    }

    pub fn remove_node(&mut self, node_id: &str) -> Option<Node> {
        if let Some(pos) = self.nodes.iter().position(|n| n.id == node_id) {
            let _ = self.disconnect(node_id);
            Some(self.nodes.remove(pos))
        } else {
            None
        }
    }

    pub fn get_node(&self, node_id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == node_id)
    }

    pub fn get_node_mut(&mut self, node_id: &str) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == node_id)
    }

    pub fn connect(&mut self, output_id: &str, input_id: &str) -> Result<(), String> {
        if output_id == input_id {
            return Err("A node cannot connect to itself".to_string());
        }

        let output_index = self.nodes.iter().position(|node| node.id == output_id)
            .ok_or_else(|| format!("Output node '{}' not found", output_id))?;
        let input_index = self.nodes.iter().position(|node| node.id == input_id)
            .ok_or_else(|| format!("Input node '{}' not found", input_id))?;

        if self.nodes[output_index].outputs.is_empty() {
            return Err("Output node has no outputs".to_string());
        }
        if self.nodes[input_index].inputs.is_empty() {
            return Err("Input node has no inputs".to_string());
        }

        self.disconnect(output_id)?;
        self.disconnect(input_id)?;
        self.nodes[output_index].outputs[0].connected_to = Some(input_id.to_string());
        self.nodes[input_index].inputs[0].connected_to = Some(output_id.to_string());

        Ok(())
    }

    pub fn disconnect(&mut self, node_id: &str) -> Result<(), String> {
        if !self.nodes.iter().any(|node| node.id == node_id) {
            return Err(format!("Node '{}' not found", node_id));
        }

        for node in &mut self.nodes {
            for input in &mut node.inputs {
                if node.id == node_id || input.connected_to.as_deref() == Some(node_id) {
                    input.connected_to = None;
                }
            }
            for output in &mut node.outputs {
                if node.id == node_id || output.connected_to.as_deref() == Some(node_id) {
                    output.connected_to = None;
                }
            }
        }

        Ok(())
    }

    pub fn set_parameter(&mut self, node_id: &str, param_name: &str, value: ParameterValue) -> Result<(), String> {
        let node = self.get_node_mut(node_id)
            .ok_or(format!("Node '{}' not found", node_id))?;

        if let Some(param) = node.parameters.iter_mut().find(|p| p.name == param_name) {
            param.value = value;
            Ok(())
        } else {
            Err(format!("Parameter '{}' not found", param_name))
        }
    }

    pub fn add_keyframe(&mut self, node_id: &str, param_name: &str, time: f64, value: ParameterValue) -> Result<(), String> {
        if !time.is_finite() {
            return Err("Keyframe time must be finite".to_string());
        }
        let node = self.get_node_mut(node_id)
            .ok_or(format!("Node '{}' not found", node_id))?;

        if let Some(param) = node.parameters.iter_mut().find(|p| p.name == param_name) {
            param.keyframes.push(Keyframe {
                time,
                value,
                interpolation: Interpolation::Linear,
            });
            param.keyframes.sort_by(|a, b| a.time.total_cmp(&b.time));
            Ok(())
        } else {
            Err(format!("Parameter '{}' not found", param_name))
        }
    }

    pub fn evaluate_parameter_at(&self, node_id: &str, param_name: &str, time: f64) -> Option<ParameterValue> {
        let node = self.get_node(node_id)?;
        let param = node.parameters.iter().find(|p| p.name == param_name)?;

        if param.keyframes.is_empty() {
            return Some(param.value.clone());
        }

        let mut prev_kf = &param.keyframes[0];
        for kf in &param.keyframes {
            if kf.time <= time {
                prev_kf = kf;
            } else {
                break;
            }
        }

        Some(prev_kf.value.clone())
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn default_ports(node_type: NodeType) -> (Vec<NodeInput>, Vec<NodeOutput>) {
        let inputs = if node_type == NodeType::Input {
            Vec::new()
        } else {
            vec![NodeInput { name: "input".to_string(), connected_to: None }]
        };
        let outputs = if node_type == NodeType::Output {
            Vec::new()
        } else {
            vec![NodeOutput { name: "output".to_string(), connected_to: None }]
        };
        (inputs, outputs)
    }

    fn default_parameters(node_type: &NodeType) -> Vec<Parameter> {
        match node_type {
            NodeType::Transform => vec![
                Parameter {
                    name: "position_x".to_string(),
                    value: ParameterValue::Float(0.0),
                    min: -1000.0,
                    max: 1000.0,
                    default: 0.0,
                    keyframes: Vec::new(),
                },
                Parameter {
                    name: "position_y".to_string(),
                    value: ParameterValue::Float(0.0),
                    min: -1000.0,
                    max: 1000.0,
                    default: 0.0,
                    keyframes: Vec::new(),
                },
                Parameter {
                    name: "scale".to_string(),
                    value: ParameterValue::Float(1.0),
                    min: 0.01,
                    max: 10.0,
                    default: 1.0,
                    keyframes: Vec::new(),
                },
                Parameter {
                    name: "rotation".to_string(),
                    value: ParameterValue::Float(0.0),
                    min: -360.0,
                    max: 360.0,
                    default: 0.0,
                    keyframes: Vec::new(),
                },
            ],
            NodeType::ColorCorrection => vec![
                Parameter {
                    name: "exposure".to_string(),
                    value: ParameterValue::Float(0.0),
                    min: -5.0,
                    max: 5.0,
                    default: 0.0,
                    keyframes: Vec::new(),
                },
                Parameter {
                    name: "contrast".to_string(),
                    value: ParameterValue::Float(0.0),
                    min: -1.0,
                    max: 1.0,
                    default: 0.0,
                    keyframes: Vec::new(),
                },
                Parameter {
                    name: "saturation".to_string(),
                    value: ParameterValue::Float(1.0),
                    min: 0.0,
                    max: 3.0,
                    default: 1.0,
                    keyframes: Vec::new(),
                },
                Parameter {
                    name: "temperature".to_string(),
                    value: ParameterValue::Float(0.0),
                    min: -100.0,
                    max: 100.0,
                    default: 0.0,
                    keyframes: Vec::new(),
                },
            ],
            NodeType::Blur => vec![
                Parameter {
                    name: "radius".to_string(),
                    value: ParameterValue::Float(0.0),
                    min: 0.0,
                    max: 100.0,
                    default: 0.0,
                    keyframes: Vec::new(),
                },
            ],
            _ => Vec::new(),
        }
    }
}

impl Default for RenderGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_node() {
        let mut graph = RenderGraph::new();
        let id = graph.add_node(NodeType::Transform, "Transform 1");
        assert_eq!(graph.node_count(), 1);
        assert!(id.starts_with("node_"));
    }

    #[test]
    fn test_set_parameter() {
        let mut graph = RenderGraph::new();
        let id = graph.add_node(NodeType::Transform, "T1");

        graph.set_parameter(&id, "scale", ParameterValue::Float(2.0)).unwrap();
        let node = graph.get_node(&id).unwrap();
        let param = node.parameters.iter().find(|p| p.name == "scale").unwrap();
        if let ParameterValue::Float(v) = &param.value {
            assert_eq!(*v, 2.0);
        } else {
            panic!("Expected Float");
        }
    }

    #[test]
    fn test_keyframe_evaluation() {
        let mut graph = RenderGraph::new();
        let id = graph.add_node(NodeType::Transform, "T1");

        graph.add_keyframe(&id, "scale", 0.0, ParameterValue::Float(1.0)).unwrap();
        graph.add_keyframe(&id, "scale", 10.0, ParameterValue::Float(2.0)).unwrap();

        let val = graph.evaluate_parameter_at(&id, "scale", 5.0).unwrap();
        if let ParameterValue::Float(v) = val {
            assert_eq!(v, 1.0);
        }
    }

    #[test]
    fn test_connect_nodes() {
        let mut graph = RenderGraph::new();
        let input = graph.add_node(NodeType::Input, "Input");
        let transform = graph.add_node(NodeType::Transform, "Transform");

        graph.connect(&input, &transform).unwrap();
        let input_node = graph.get_node(&input).unwrap();
        assert!(input_node.outputs[0].connected_to.is_some());
        graph.disconnect(&input).unwrap();
        assert!(graph.get_node(&transform).unwrap().inputs[0].connected_to.is_none());
    }

    #[test]
    fn test_remove_node() {
        let mut graph = RenderGraph::new();
        let id = graph.add_node(NodeType::Blur, "Blur 1");
        assert_eq!(graph.node_count(), 1);

        graph.remove_node(&id);
        assert_eq!(graph.node_count(), 0);
    }
}
