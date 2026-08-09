use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};

use super::checkpoint::Checkpoint;
use super::engine::Graph;
use super::state::GraphState;

#[derive(Debug)]
pub enum ExecutionCommand {
    Pause,
    Resume,
    Stop,
    Inspect(oneshot::Sender<ExecutionSnapshot>),
}

#[derive(Debug, Clone)]
pub struct ExecutionSnapshot {
    pub run_id: String,
    pub current_step: usize,
    pub current_nodes: Vec<String>,
    pub execution_path: Vec<String>,
    pub current_state: GraphState,
    pub status: ExecutionStatus,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionStatus {
    Running,
    Paused,
    Completed,
    Failed,
    Stopped,
}

pub struct ExecutionContext {
    run_id: String,
    step: usize,
    current_nodes: Vec<String>,
    execution_path: Vec<String>,
    state: GraphState,
    status: ExecutionStatus,
    last_error: Option<String>,
}

impl ExecutionContext {
    pub fn new(run_id: String, state: GraphState) -> Self {
        Self {
            run_id,
            step: 0,
            current_nodes: Vec::new(),
            execution_path: Vec::new(),
            state,
            status: ExecutionStatus::Running,
            last_error: None,
        }
    }

    pub fn snapshot(&self) -> ExecutionSnapshot {
        ExecutionSnapshot {
            run_id: self.run_id.clone(),
            current_step: self.step,
            current_nodes: self.current_nodes.clone(),
            execution_path: self.execution_path.clone(),
            current_state: self.state.clone(),
            status: self.status.clone(),
            last_error: self.last_error.clone(),
        }
    }
}

pub struct GraphExecutionHandle {
    command_tx: mpsc::Sender<ExecutionCommand>,
    context: Arc<RwLock<ExecutionContext>>,
}

impl GraphExecutionHandle {
    pub async fn pause(&self) -> Result<(), String> {
        self.command_tx
            .send(ExecutionCommand::Pause)
            .await
            .map_err(|e| format!("发送暂停命令失败: {}", e))
    }

    pub async fn resume(&self) -> Result<(), String> {
        self.command_tx
            .send(ExecutionCommand::Resume)
            .await
            .map_err(|e| format!("发送恢复命令失败: {}", e))
    }

    pub async fn stop(&self) -> Result<(), String> {
        self.command_tx
            .send(ExecutionCommand::Stop)
            .await
            .map_err(|e| format!("发送停止命令失败: {}", e))
    }

    pub async fn inspect(&self) -> Result<ExecutionSnapshot, String> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(ExecutionCommand::Inspect(tx))
            .await
            .map_err(|e| format!("发送检查命令失败: {}", e))?;

        rx.await.map_err(|e| format!("获取检查结果失败: {}", e))
    }

    pub async fn status(&self) -> ExecutionStatus {
        let ctx = self.context.read().await;
        ctx.status.clone()
    }
}

pub struct GraphExecution;

impl GraphExecution {
    pub async fn spawn(
        graph: Arc<Graph>,
        state: GraphState,
        run_id: Option<String>,
    ) -> GraphExecutionHandle {
        let run_id =
            run_id.unwrap_or_else(|| format!("run_{}", chrono::Utc::now().timestamp_millis()));

        let (command_tx, mut command_rx) = mpsc::channel(16);
        let context = Arc::new(RwLock::new(ExecutionContext::new(
            run_id.clone(),
            state.clone(),
        )));
        let context_clone = context.clone();

        tokio::spawn(async move {
            let mut current_state = state;
            let mut step = 0;
            let mut execution_path = Vec::new();
            let mut current_nodes = Vec::new();
            let mut _final_output = String::new();
            let mut success = true;
            let mut last_error: Option<String> = None;

            let entry = match graph.entry.as_ref() {
                Some(e) => e,
                None => {
                    *context_clone.write().await = ExecutionContext {
                        run_id: run_id.clone(),
                        step,
                        current_nodes,
                        execution_path,
                        state: current_state,
                        status: ExecutionStatus::Failed,
                        last_error: Some("入口节点未设置".to_string()),
                    };
                    return;
                }
            };

            current_nodes = vec![entry.clone()];

            loop {
                if let Ok(cmd) = command_rx.try_recv() {
                    match cmd {
                        ExecutionCommand::Pause => {
                            tracing::info!("执行暂停: run_id={}", run_id);
                            Self::save_checkpoint(
                                &graph,
                                &run_id,
                                &current_state,
                                step,
                                &current_nodes,
                            )
                            .await;

                            *context_clone.write().await = ExecutionContext {
                                run_id: run_id.clone(),
                                step,
                                current_nodes: current_nodes.clone(),
                                execution_path: execution_path.clone(),
                                state: current_state.clone(),
                                status: ExecutionStatus::Paused,
                                last_error: last_error.clone(),
                            };

                            loop {
                                match command_rx.recv().await {
                                    Some(ExecutionCommand::Resume) => {
                                        tracing::info!("执行恢复: run_id={}", run_id);
                                        current_state = Self::load_checkpoint(&graph, &run_id)
                                            .await
                                            .unwrap_or(current_state);
                                        *context_clone.write().await = ExecutionContext {
                                            run_id: run_id.clone(),
                                            step,
                                            current_nodes: current_nodes.clone(),
                                            execution_path: execution_path.clone(),
                                            state: current_state.clone(),
                                            status: ExecutionStatus::Running,
                                            last_error: last_error.clone(),
                                        };
                                        break;
                                    }
                                    Some(ExecutionCommand::Stop) => {
                                        tracing::info!("执行停止: run_id={}", run_id);
                                        *context_clone.write().await = ExecutionContext {
                                            run_id: run_id.clone(),
                                            step,
                                            current_nodes: current_nodes.clone(),
                                            execution_path: execution_path.clone(),
                                            state: current_state.clone(),
                                            status: ExecutionStatus::Stopped,
                                            last_error: last_error.clone(),
                                        };
                                        return;
                                    }
                                    Some(ExecutionCommand::Inspect(tx)) => {
                                        let snapshot = ExecutionSnapshot {
                                            run_id: run_id.clone(),
                                            current_step: step,
                                            current_nodes: current_nodes.clone(),
                                            execution_path: execution_path.clone(),
                                            current_state: current_state.clone(),
                                            status: ExecutionStatus::Paused,
                                            last_error: last_error.clone(),
                                        };
                                        let _ = tx.send(snapshot);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        ExecutionCommand::Stop => {
                            tracing::info!("执行停止: run_id={}", run_id);
                            *context_clone.write().await = ExecutionContext {
                                run_id: run_id.clone(),
                                step,
                                current_nodes: current_nodes.clone(),
                                execution_path: execution_path.clone(),
                                state: current_state.clone(),
                                status: ExecutionStatus::Stopped,
                                last_error: last_error.clone(),
                            };
                            return;
                        }
                        ExecutionCommand::Inspect(tx) => {
                            let snapshot = ExecutionSnapshot {
                                run_id: run_id.clone(),
                                current_step: step,
                                current_nodes: current_nodes.clone(),
                                execution_path: execution_path.clone(),
                                current_state: current_state.clone(),
                                status: ExecutionStatus::Running,
                                last_error: last_error.clone(),
                            };
                            let _ = tx.send(snapshot);
                        }
                        _ => {}
                    }
                }

                if current_nodes.is_empty() {
                    break;
                }

                step += 1;

                let mut pending = Vec::new();
                for node_name in &current_nodes {
                    if let Some(node) = graph.nodes.get(node_name) {
                        let state_clone = current_state.clone();
                        pending.push((node_name.clone(), node.func.clone(), state_clone));
                    }
                }

                let mut results = Vec::new();
                for (node_name, func, state_clone) in pending {
                    let result = func.call(state_clone).await;
                    results.push((node_name, result));
                }

                let mut next_nodes = Vec::new();
                for (node_name, result) in results {
                    execution_path.push(node_name.clone());

                    if !result.success {
                        success = false;
                        last_error = result.error.clone();
                        break;
                    }

                    let route = result.route.clone();
                    let output = result.output.clone();

                    if !result.state_updates.is_empty() {
                        current_state.merge_with_reducers(result.state_updates, &graph.reducers);
                    }

                    let _ = output;

                    let nexts =
                        graph.determine_next_all(&node_name, &current_state, route.as_ref());
                    next_nodes.extend(nexts);
                }

                if !success {
                    break;
                }

                let mut seen = std::collections::HashSet::new();
                current_nodes = next_nodes
                    .into_iter()
                    .filter(|n| seen.insert(n.clone()))
                    .collect();

                *context_clone.write().await = ExecutionContext {
                    run_id: run_id.clone(),
                    step,
                    current_nodes: current_nodes.clone(),
                    execution_path: execution_path.clone(),
                    state: current_state.clone(),
                    status: ExecutionStatus::Running,
                    last_error: last_error.clone(),
                };
            }

            let status = if success {
                ExecutionStatus::Completed
            } else {
                ExecutionStatus::Failed
            };

            *context_clone.write().await = ExecutionContext {
                run_id: run_id.clone(),
                step,
                current_nodes,
                execution_path,
                state: current_state.clone(),
                status,
                last_error,
            };
        });

        GraphExecutionHandle {
            command_tx,
            context,
        }
    }

    async fn save_checkpoint(
        graph: &Graph,
        run_id: &str,
        state: &GraphState,
        step: usize,
        current_nodes: &[String],
    ) {
        if let Some(checkpoint) = current_nodes.first() {
            let cp = Checkpoint {
                id: format!("checkpoint_{}", step),
                run_id: run_id.to_string(),
                completed_node: checkpoint.clone(),
                next_node: current_nodes.first().cloned(),
                state: state.clone(),
                timestamp: chrono::Utc::now(),
                step,
            };
            if let Err(e) = graph.checkpoint_store.save(cp).await {
                tracing::warn!("保存检查点失败: {}", e);
            }
        }
    }

    async fn load_checkpoint(graph: &Graph, run_id: &str) -> Option<GraphState> {
        match graph.checkpoint_store.get_latest(run_id).await {
            Some(cp) => {
                tracing::info!("从检查点恢复: step={}", cp.step);
                Some(cp.state)
            }
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::engine::GraphBuilder;
    use super::super::node::NodeResult;
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn test_execution_pause_resume() {
        let shared_count = Arc::new(AtomicUsize::new(0));
        let counter = shared_count.clone();

        let graph = GraphBuilder::new()
            .node("a", move |_| {
                let cnt = counter.clone();
                async move {
                    cnt.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    NodeResult::ok("ok_a")
                }
            })
            .entry("a")
            .build()
            .unwrap();

        let handle = GraphExecution::spawn(Arc::new(graph), GraphState::new(), None).await;

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert_eq!(handle.status().await, ExecutionStatus::Running);

        handle.pause().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        assert_eq!(handle.status().await, ExecutionStatus::Paused);

        let snapshot = handle.inspect().await.unwrap();
        assert!(snapshot.execution_path.contains(&"a".to_string()));

        handle.resume().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        assert_eq!(handle.status().await, ExecutionStatus::Completed);
        assert_eq!(shared_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_execution_stop() {
        let shared_count = Arc::new(AtomicUsize::new(0));
        let counter = shared_count.clone();

        let graph = GraphBuilder::new()
            .node("a", move |_| {
                let cnt = counter.clone();
                async move {
                    cnt.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    NodeResult::ok("ok_a")
                }
            })
            .entry("a")
            .build()
            .unwrap();

        let handle = GraphExecution::spawn(Arc::new(graph), GraphState::new(), None).await;

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        handle.stop().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        assert_eq!(handle.status().await, ExecutionStatus::Stopped);
    }

    #[tokio::test]
    async fn test_execution_inspect_running() {
        let graph = GraphBuilder::new()
            .node("a", |_| async {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                NodeResult::ok("ok_a")
            })
            .node("b", |_| async {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                NodeResult::ok("ok_b")
            })
            .edge("a", "b")
            .entry("a")
            .build()
            .unwrap();

        let handle = GraphExecution::spawn(Arc::new(graph), GraphState::new(), None).await;

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let snapshot = handle.inspect().await.unwrap();
        assert_eq!(snapshot.status, ExecutionStatus::Running);
        assert!(!snapshot.execution_path.is_empty());
    }
}
