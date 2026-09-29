import {
  AppShell,
  Badge,
  Box,
  CairnTheme,
  Flex,
  Heading,
  LegalPage,
  PageBar,
  Text,
  type CairnAppearance,
} from "@cairn/ui";
import { tauriDragRegion } from "@cairn/host-tauri";
import { useEffect, useState } from "react";
import { Applications } from "./applications.js";
import { activateTitleBar, readSnapshot } from "./backend.js";
import { navigate, useRoute } from "./navigation.js";
import { messageOf, useResource } from "./resource.js";
import { Settings } from "./settings.js";
import { Workflows } from "./workflows.js";

function savedAppearance(): CairnAppearance {
  try {
    const value = localStorage.getItem("flint.appearance");
    return value === "light" || value === "dark" ? value : "inherit";
  } catch {
    return "inherit";
  }
}

export function App() {
  const route = useRoute();
  const snapshot = useResource("snapshot", readSnapshot, 1500);
  const [appearance, setAppearance] = useState(savedAppearance);
  const [preferenceError, setPreferenceError] = useState("");
  const [chrome, setChrome] = useState(false);
  useEffect(() => {
    let active = true;
    activateTitleBar().then(
      (mode) => {
        if (active) setChrome(mode === "custom");
      },
      () => {},
    );
    return () => {
      active = false;
    };
  }, []);
  const updateAppearance = (value: CairnAppearance) => {
    setAppearance(value);
    try {
      localStorage.setItem("flint.appearance", value);
      setPreferenceError("");
    } catch (error) {
      setPreferenceError(`Could not save appearance: ${messageOf(error)}`);
    }
  };
  const ready = snapshot.data?.backend.ready === true && !snapshot.error;
  const instances = snapshot.data?.instances ?? [];
  return (
    <CairnTheme appearance={appearance}>
      <AppShell.Root
        scroll="panes"
        windowChrome={chrome ? tauriDragRegion : undefined}
      >
        {!chrome ? (
          <AppShell.Header>
            <Flex align="center" gap="3">
              <AppShell.SidebarToggle />
              <Heading size="body-large">Flint</Heading>
            </Flex>
          </AppShell.Header>
        ) : null}
        <AppShell.Sidebar collapsible label="Flint navigation">
          <AppShell.NavGroup>
            <AppShell.NavItem
              active={route.page === "apps"}
              href="#/apps"
              icon="app-window"
            >
              Applications
            </AppShell.NavItem>
            <AppShell.NavItem
              active={route.page === "workflows"}
              href="#/workflows"
              icon="layers"
            >
              Workflows
            </AppShell.NavItem>
          </AppShell.NavGroup>
          <AppShell.SidebarFooter>
            <AppShell.NavItem
              active={route.page === "settings" || route.page === "legal"}
              href="#/settings"
              icon="settings"
            >
              Settings
            </AppShell.NavItem>
            <Box px="3" py="3">
              <Flex direction="column" align="start" gap="2">
                <Badge tone={ready ? "success" : "warning"}>
                  {ready
                    ? "Backend running"
                    : snapshot.loading && !snapshot.data
                      ? "Connecting"
                      : "Backend unavailable"}
                </Badge>
                <Text size="caption" tone="muted">
                  {instances.length} connected
                </Text>
              </Flex>
            </Box>
          </AppShell.SidebarFooter>
        </AppShell.Sidebar>
        <AppShell.Main>
          {route.page === "apps" ? (
            <Applications
              route={route}
              snapshot={snapshot.data}
              loading={snapshot.loading}
              error={snapshot.error}
              refresh={snapshot.reload}
            />
          ) : null}
          {route.page === "workflows" ? (
            <Workflows selectedId={route.id} instances={instances} />
          ) : null}
          {route.page === "settings" ? (
            <Settings
              appearance={appearance}
              onAppearanceChange={updateAppearance}
              preferenceError={preferenceError}
              ready={ready}
            />
          ) : null}
          {route.page === "legal" ? (
            <>
              <PageBar.Root>
                <PageBar.Back
                  label="Back to settings"
                  onSelect={() => navigate("settings")}
                />
                <PageBar.Title>Licenses</PageBar.Title>
              </PageBar.Root>
              <LegalPage embedded />
            </>
          ) : null}
        </AppShell.Main>
      </AppShell.Root>
    </CairnTheme>
  );
}
