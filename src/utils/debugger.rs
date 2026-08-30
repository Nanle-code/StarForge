use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    pub var_type: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackFrame {
    pub function: String,
    pub contract_id: Option<String>,
    pub source_location: Option<String>,
    pub variables: Vec<Variable>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Breakpoint {
    pub id: usize,
    pub contract_id: Option<String>,
    pub function: String,
    pub condition: Option<String>,
    pub enabled: bool,
    pub hit_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionState {
    Running,
    Paused,
    SteppingInto,
    SteppingOver,
    SteppingOut,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugSession {
    pub contract_id: Option<String>,
    pub wasm_path: Option<String>,
    pub network: String,
    pub state: ExecutionState,
    pub breakpoints: Vec<Breakpoint>,
    pub call_stack: Vec<StackFrame>,
    pub variables: Vec<Variable>,
    pub current_function: Option<String>,
    pub step_count: u64,
    pub history: Vec<String>,
}

impl DebugSession {
    pub fn new(contract_id: Option<String>, wasm_path: Option<String>, network: &str) -> Self {
        Self {
            contract_id,
            wasm_path,
            network: network.to_string(),
            state: ExecutionState::Running,
            breakpoints: Vec::new(),
            call_stack: Vec::new(),
            variables: Vec::new(),
            current_function: None,
            step_count: 0,
            history: Vec::new(),
        }
    }

    pub fn pause(&mut self) {
        self.state = ExecutionState::Paused;
    }

    pub fn resume(&mut self) {
        self.state = ExecutionState::Running;
    }

    pub fn stop(&mut self) {
        self.state = ExecutionState::Stopped;
    }

    pub fn is_paused(&self) -> bool {
        matches!(
            self.state,
            ExecutionState::Paused
                | ExecutionState::SteppingInto
                | ExecutionState::SteppingOver
                | ExecutionState::SteppingOut
        )
    }

    pub fn add_step_history(&mut self, entry: String) {
        self.step_count += 1;
        self.history
            .push(format!("#{}: {}", self.step_count, entry));
    }

    pub fn set_variables(&mut self, vars: Vec<Variable>) {
        self.variables = vars;
    }

    pub fn set_call_stack(&mut self, frames: Vec<StackFrame>) {
        self.call_stack = frames;
    }

    pub fn push_frame(&mut self, frame: StackFrame) {
        self.current_function = Some(frame.function.clone());
        self.call_stack.push(frame);
    }

    pub fn pop_frame(&mut self) -> Option<StackFrame> {
        let frame = self.call_stack.pop();
        self.current_function = self.call_stack.last().map(|f| f.function.clone());
        frame
    }
}

pub struct Debugger {
    pub session: DebugSession,
    next_breakpoint_id: usize,
}

impl Debugger {
    pub fn new(contract_id: Option<String>, wasm_path: Option<String>, network: &str) -> Self {
        Self {
            session: DebugSession::new(contract_id, wasm_path, network),
            next_breakpoint_id: 1,
        }
    }

    pub fn add_breakpoint(
        &mut self,
        contract_id: Option<String>,
        function: &str,
        condition: Option<String>,
    ) -> &Breakpoint {
        let bp = Breakpoint {
            id: self.next_breakpoint_id,
            contract_id,
            function: function.to_string(),
            condition,
            enabled: true,
            hit_count: 0,
        };
        self.next_breakpoint_id += 1;
        self.session.breakpoints.push(bp);
        self.session.breakpoints.last().unwrap()
    }

    pub fn remove_breakpoint(&mut self, id: usize) -> bool {
        let len_before = self.session.breakpoints.len();
        self.session.breakpoints.retain(|bp| bp.id != id);
        self.session.breakpoints.len() < len_before
    }

    pub fn enable_breakpoint(&mut self, id: usize) -> bool {
        if let Some(bp) = self.session.breakpoints.iter_mut().find(|bp| bp.id == id) {
            bp.enabled = true;
            true
        } else {
            false
        }
    }

    pub fn disable_breakpoint(&mut self, id: usize) -> bool {
        if let Some(bp) = self.session.breakpoints.iter_mut().find(|bp| bp.id == id) {
            bp.enabled = false;
            true
        } else {
            false
        }
    }

    pub fn list_breakpoints(&self) -> &[Breakpoint] {
        &self.session.breakpoints
    }

    pub fn should_break(&mut self, contract_id: &str, function: &str) -> Option<usize> {
        for i in 0..self.session.breakpoints.len() {
            if !self.session.breakpoints[i].enabled {
                continue;
            }
            let contract_matches = self.session.breakpoints[i]
                .contract_id
                .as_ref()
                .map_or(true, |cid| cid == contract_id);
            if contract_matches && self.session.breakpoints[i].function == function {
                if let Some(ref condition) = self.session.breakpoints[i].condition {
                    if !evaluate_condition(condition) {
                        continue;
                    }
                }
                self.session.breakpoints[i].hit_count += 1;
                return Some(self.session.breakpoints[i].id);
            }
        }
        None
    }

    fn evaluate_condition(&self, _condition: &str) -> bool {
        true
    }

    pub fn step_into(&mut self) {
        self.session.state = ExecutionState::SteppingInto;
    }

    pub fn step_over(&mut self) {
        self.session.state = ExecutionState::SteppingOver;
    }

    pub fn step_out(&mut self) {
        self.session.state = ExecutionState::SteppingOut;
    }

    pub fn continue_execution(&mut self) {
        self.session.state = ExecutionState::Running;
    }

    pub fn inspect_variable(&self, name: &str) -> Option<&Variable> {
        self.session
            .variables
            .iter()
            .find(|v| v.name == name || v.name.contains(name))
    }

    pub fn inspect_all_variables(&self) -> &[Variable] {
        &self.session.variables
    }

    pub fn inspect_call_stack(&self) -> &[StackFrame] {
        &self.session.call_stack
    }

    pub fn find_variable(&self, name: &str) -> Option<&Variable> {
        for frame in self.session.call_stack.iter().rev() {
            if let Some(var) = frame.variables.iter().find(|v| v.name == name) {
                return Some(var);
            }
        }
        self.session.variables.iter().find(|v| v.name == name)
    }

    pub fn search_variables(&self, pattern: &str) -> Vec<&Variable> {
        let lower = pattern.to_lowercase();
        let mut results: Vec<&Variable> = self
            .session
            .variables
            .iter()
            .filter(|v| {
                v.name.to_lowercase().contains(&lower)
                    || v.value.to_lowercase().contains(&lower)
                    || v.var_type.to_lowercase().contains(&lower)
            })
            .collect();
        for frame in &self.session.call_stack {
            for v in &frame.variables {
                if !results.iter().any(|r| std::ptr::eq(*r, v))
                    && (v.name.to_lowercase().contains(&lower)
                        || v.value.to_lowercase().contains(&lower)
                        || v.var_type.to_lowercase().contains(&lower))
                {
                    results.push(v);
                }
            }
        }
        results
    }
}

fn evaluate_condition(_condition: &str) -> bool {
    true
}

pub fn parse_variable_from_output(output: &str) -> Vec<Variable> {
    let mut vars = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((name, rest)) = line.split_once(':') {
            let name = name.trim().to_string();
            let rest = rest.trim();
            if let Some((var_type, value)) = rest.split_once('=') {
                vars.push(Variable {
                    name,
                    var_type: var_type.trim().to_string(),
                    value: value.trim().to_string(),
                });
            } else {
                vars.push(Variable {
                    name,
                    var_type: "unknown".to_string(),
                    value: rest.to_string(),
                });
            }
        } else {
            vars.push(Variable {
                name: format!("var_{}", vars.len()),
                var_type: "unknown".to_string(),
                value: line.to_string(),
            });
        }
    }
    vars
}
