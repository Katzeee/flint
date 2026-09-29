import {
  Badge,
  Box,
  Card,
  EmptyState,
  Flex,
  Heading,
  List,
  ListDetail,
  PageBar,
  Tabs,
  Text,
  TextArea,
} from "@cairn/ui";
import { useState } from "react";
import {
  readWorkflow,
  readWorkflows,
  type ConnectedInstance,
  type Execution,
} from "./backend.js";
import { navigate, workflowPath } from "./navigation.js";
import { useResource } from "./resource.js";
import { ErrorNotice, formatTime, Loading } from "./shared.js";

function ExecutionRecord({
  execution,
  instances,
}: Readonly<{
  execution: Execution;
  instances: readonly ConnectedInstance[];
}>) {
  const [expanded, setExpanded] = useState(false);
  const instance = instances.find(
    (item) => item.instance_id === execution.instance_id,
  );
  const status = execution.status;
  return (
    <Box>
      <List.Root>
        <List.Item
          aria-expanded={expanded}
          onClick={() => setExpanded(!expanded)}
          description={`${instance?.instance_name ?? execution.instance_id} · ${formatTime(execution.started_at)}`}
          trailing={
            <Badge
              tone={
                status === "succeeded"
                  ? "success"
                  : status === "failed"
                    ? "danger"
                    : "neutral"
              }
            >
              {status}
            </Badge>
          }
        >
          {execution.name || `Execution ${execution.execution_id}`}
        </List.Item>
      </List.Root>
      {expanded ? (
        <Box px="4" pb="4">
          <Card>
            <Tabs.Root defaultValue="output">
              <Tabs.List aria-label="Execution details">
                <Tabs.Trigger value="output">Output</Tabs.Trigger>
                <Tabs.Trigger value="code">Code</Tabs.Trigger>
              </Tabs.List>
              <Tabs.Content value="output">
                <Flex direction="column" gap="3" pt="3">
                  {execution.error ? (
                    <ErrorNotice error={execution.error} />
                  ) : null}
                  {execution.stdout ? (
                    <TextArea
                      aria-label="Standard output"
                      readOnly
                      value={execution.stdout}
                      rows={8}
                    />
                  ) : null}
                  {execution.stderr || execution.traceback ? (
                    <TextArea
                      aria-label="Error output"
                      readOnly
                      value={[execution.stderr, execution.traceback]
                        .filter(Boolean)
                        .join("\n")}
                      rows={8}
                    />
                  ) : null}
                  {!execution.stdout &&
                  !execution.stderr &&
                  !execution.traceback &&
                  !execution.error ? (
                    <Text tone="muted">
                      {status === "running" || status === "pending"
                        ? "Waiting for output…"
                        : "No output recorded."}
                    </Text>
                  ) : null}
                </Flex>
              </Tabs.Content>
              <Tabs.Content value="code">
                <Box pt="3">
                  <TextArea
                    aria-label="Executed code"
                    readOnly
                    value={execution.code}
                    rows={12}
                  />
                </Box>
              </Tabs.Content>
            </Tabs.Root>
          </Card>
        </Box>
      ) : null}
    </Box>
  );
}

export function Workflows({
  selectedId,
  instances,
}: Readonly<{ selectedId?: string; instances: readonly ConnectedInstance[] }>) {
  const index = useResource("workflows", readWorkflows, 3000);
  const selected = useResource(
    selectedId ? `workflow:${selectedId}` : null,
    () => readWorkflow(selectedId!),
    1500,
  );
  return (
    <ListDetail.Root
      pane={selectedId ? "detail" : "list"}
      onPaneChange={(pane) => {
        if (pane === "list") navigate("workflows");
      }}
    >
      <ListDetail.List label="Workflows">
        <PageBar.Root>
          <PageBar.Title>Workflows</PageBar.Title>
          <PageBar.Action
            icon="compass"
            label="Refresh workflows"
            onSelect={index.reload}
          />
        </PageBar.Root>
        {index.error ? (
          <Box p="4">
            <ErrorNotice error={index.error} retry={index.reload} />
          </Box>
        ) : null}
        {index.loading && !index.data ? <Loading /> : null}
        {index.data?.length === 0 ? (
          <Box p="5">
            <EmptyState>
              <EmptyState.Title>No workflows yet</EmptyState.Title>
              <EmptyState.Description>
                Workflows created through Flint appear here with their execution
                records.
              </EmptyState.Description>
            </EmptyState>
          </Box>
        ) : null}
        <List.Root>
          {index.data?.map((item) => (
            <List.Item
              key={item.workflow_id}
              selected={selectedId === item.workflow_id}
              onClick={() => navigate(workflowPath(item.workflow_id))}
              description={`${item.execution_count} executions · ${formatTime(item.updated_at)}`}
              trailing={
                item.running_count > 0 ? (
                  <Badge>{item.running_count} running</Badge>
                ) : undefined
              }
            >
              {item.name || "Untitled workflow"}
            </List.Item>
          ))}
        </List.Root>
      </ListDetail.List>
      <ListDetail.Detail label="Workflow details">
        <PageBar.Root>
          <PageBar.Title>
            {selected.data?.name || (selectedId ? "Workflow" : "Workflows")}
          </PageBar.Title>
          {selectedId ? (
            <PageBar.Action
              icon="compass"
              label="Refresh execution records"
              onSelect={selected.reload}
            />
          ) : null}
        </PageBar.Root>
        {!selectedId ? (
          <Box p="5">
            <EmptyState>
              <EmptyState.Title>Select a workflow</EmptyState.Title>
              <EmptyState.Description>
                Inspect its executions, code, output and errors.
              </EmptyState.Description>
            </EmptyState>
          </Box>
        ) : null}
        {selected.error ? (
          <Box p="5">
            <ErrorNotice error={selected.error} retry={selected.reload} />
          </Box>
        ) : null}
        {selected.loading && !selected.data ? <Loading /> : null}
        {selected.data ? (
          <Box p="5">
            <Flex direction="column" gap="4">
              {selected.data.description ? (
                <Text as="p" tone="muted">
                  {selected.data.description}
                </Text>
              ) : null}
              <Heading size="title-small">Executions</Heading>
              {selected.data.execs.length === 0 ? (
                <Text tone="muted">No executions recorded.</Text>
              ) : (
                <Box key={selected.data.workflow_id}>
                  {selected.data.execs.map((execution) => (
                    <ExecutionRecord
                      key={execution.execution_id}
                      execution={execution}
                      instances={instances}
                    />
                  ))}
                </Box>
              )}
            </Flex>
          </Box>
        ) : null}
      </ListDetail.Detail>
    </ListDetail.Root>
  );
}
