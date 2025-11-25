use crate::app::model::{Output, PlanStepExecution, plan_step::PlanStep};

#[derive(Debug, Default)]
pub struct Plan {
    selected_idx: usize,
    commands: Vec<PlanStep>,
}

impl Plan {
    pub fn from_string(s: &str) -> Plan {
        let steps: Vec<PlanStep> = s
            .split("\n")
            .map(|step| step.trim().to_string())
            .filter(|step| !step.is_empty())
            .map(|cmd| {
                let name = cmd
                    .strip_prefix("cargo ")
                    .and_then(|s| s.split_whitespace().next())
                    .unwrap_or("custom")
                    .to_string();
                PlanStep {
                    name,
                    cmd: cmd.to_string(),
                    exec: PlanStepExecution::not_run(),
                }
            })
            .collect();
        if steps.is_empty() {
            Plan {
                selected_idx: 0,
                commands: vec![PlanStep {
                    name: "Error".to_string(),
                    cmd: "echo 1".to_string(),
                    exec: PlanStepExecution::error(
                        "The supplied plan did not contain any commands, please supply a plan in the one command per line format".to_string()
                    ),
                }],
            }
        } else {
            Plan {
                selected_idx: 0,
                commands: steps,
            }
        }
    }
}

impl Plan {
    pub fn select(&mut self, idx: usize) {
        if self.commands.get(idx).is_some() {
            self.selected_idx = idx;
        } else {
            self.selected_idx = 0;
        }
    }

    pub fn selected_idx(&self) -> usize {
        self.selected_idx
    }

    pub fn selected(&self) -> &PlanStep {
        &self.commands[self.selected_idx]
    }

    fn selected_mut(&mut self) -> &mut PlanStep {
        self.commands
            .get_mut(self.selected_idx)
            .expect("should always have a step")
    }

    pub fn current_output(&self) -> &Output {
        self.selected().output()
    }

    pub fn current_step_len(&self) -> usize {
        self.selected().cmd.len()
    }

    pub fn set_current_command(&mut self, cmd: String) {
        self.selected_mut().cmd = cmd;
    }

    pub fn start_next(&mut self) {
        if let Some(next) = self.next_step() {
            next.start();
        }
    }

    pub async fn check(&mut self) {
        for cmd in self.commands.iter_mut() {
            cmd.check().await;
        }
    }

    pub fn reset(&mut self) {
        self.selected_idx = 0;
        self.commands.iter_mut().for_each(|step| {
            step.reset();
        });
    }

    pub fn is_running(&self) -> bool {
        self.commands.iter().any(|step| step.is_running())
    }

    pub fn is_finished(&self) -> bool {
        self.commands.iter().any(|step| step.is_finished())
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn as_text(&self) -> String {
        self.commands
            .iter()
            .map(|step| step.cmd.to_string())
            .fold(String::new(), |acc, s| acc + "\n" + &s)
    }

    pub fn commands(&self) -> impl Iterator<Item = &PlanStep> {
        self.commands.iter()
    }

    pub fn select_best_step_to_present_idx(&mut self) {
        let first_running_step = self
            .commands
            .iter()
            .enumerate()
            .find_map(|(idx, step)| if step.is_running() { Some(idx) } else { None });
        let first_step_with_errors = self
            .commands
            .iter()
            .enumerate()
            .find_map(|(idx, step)| if step.has_errors() { Some(idx) } else { None });
        let first_step_with_failures = self
            .commands
            .iter()
            .enumerate()
            .find_map(|(idx, step)| if step.has_failures() { Some(idx) } else { None });
        let first_step_with_warnings = self
            .commands
            .iter()
            .enumerate()
            .find_map(|(idx, step)| if step.has_warnings() { Some(idx) } else { None });
        let last_successful_step = if self.is_running() {
            self.commands.iter().enumerate().find_map(|(idx, step)| {
                if step.is_running() && idx > 0 {
                    Some(idx - 1)
                } else {
                    None
                }
            })
        } else {
            None
        };
        let last_step = if !self.commands.is_empty() && self.is_finished() {
            Some(self.commands.len() - 1)
        } else {
            None
        };
        let first_step = if !self.commands.is_empty() {
            Some(0)
        } else {
            None
        };
        let best = first_running_step
            .or(first_step_with_errors)
            .or(first_step_with_failures)
            .or(first_step_with_warnings)
            .or(last_successful_step)
            .or(last_step)
            .or(first_step);
        if let Some(best) = best {
            self.selected_idx = best
        }
    }

    fn next_step(&mut self) -> Option<&mut PlanStep> {
        self.commands.iter_mut().find(|step| step.is_ready())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_string_empty() {
        let plan = Plan::from_string("");
        assert_eq!(plan.selected_idx, 0);
        assert_eq!(plan.commands.len(), 1);
    }

    #[test]
    fn test_from_string() {
        let plan = Plan::from_string(
            r#"
            cargo check
            cargo test
            "#,
        );
        assert_eq!(plan.selected_idx, 0);
        assert_eq!(plan.commands.len(), 2);
        let cmds: Vec<String> = plan.commands().map(|step| step.cmd.to_string()).collect();
        assert_eq!(
            cmds,
            vec!["cargo check".to_string(), "cargo test".to_string()]
        );
    }

    #[test]
    fn test_from_string_non_cargo() {
        let plan = Plan::from_string(
            r#"
            echo "Hello"
            "#,
        );
        assert_eq!(plan.selected_idx, 0);
        assert_eq!(plan.commands.len(), 1);
        let cmds: Vec<String> = plan.commands().map(|step| step.cmd.to_string()).collect();
        assert_eq!(cmds, vec![r#"echo "Hello""#.to_string()]);
    }
}
