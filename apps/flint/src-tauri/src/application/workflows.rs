//! Workflow records and execution operations.
use super::{Application, Result};
use flint_contracts::protocol::{envelope::Payload, *};

impl Application {
    pub async fn create_workflow(&self, request: StartWorkflowRequest) -> Result<StartWorkflowResponse> {
        self.query(Payload::StartWorkflowRequest(request), |response| match response {
            Payload::StartWorkflowResponse(response) => Some(response),
            _ => None,
        })
        .await
    }

    pub async fn workflows(&self) -> Result<Vec<WorkflowSummary>> {
        self.query(
            Payload::ListWorkflowsRequest(ListWorkflowsRequest {}),
            |response| match response {
                Payload::ListWorkflowsResponse(response) => Some(response.workflows),
                _ => None,
            },
        )
        .await
    }

    pub async fn workflow(&self, workflow_id: String) -> Result<Workflow> {
        self.query(
            Payload::GetWorkflowRequest(GetWorkflowRequest { workflow_id }),
            |response| match response {
                Payload::GetWorkflowResponse(response) => response.workflow,
                _ => None,
            },
        )
        .await
    }

    /// Code and its optional filename are already supplied; terminal and file input belong to the caller.
    pub async fn execute(&self, request: ExecuteRequest) -> Result<ExecuteResponse> {
        self.query(Payload::ExecuteRequest(request), |response| match response {
            Payload::ExecuteResponse(response) => Some(response),
            _ => None,
        })
        .await
    }

    pub async fn execution(&self, workflow_id: String, execution_id: String, view: ExecutionView) -> Result<Execution> {
        self.query(
            Payload::GetExecutionRequest(GetExecutionRequest {
                workflow_id,
                execution_id,
                view: view as i32,
            }),
            |response| match response {
                Payload::GetExecutionResponse(response) => response.execution,
                _ => None,
            },
        )
        .await
    }
}
