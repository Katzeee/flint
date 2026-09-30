import {
  AppShell,
  Callout,
  CairnTheme,
  Flex,
  Heading,
  Icon,
  LegalPage,
  PageBar,
  type CairnAppearance,
} from "@cairn/ui";
import { tauriDragRegion } from "@cairn/host-tauri";
import { AppWindow, CircleAlert, Layers, LoaderCircle, Settings as SettingsGlyph } from "lucide-react";
import { useEffect, useState } from "react";
import { Applications } from "./applications.js";
import { activateTitleBar, discoverHosts, readSnapshot } from "./backend.js";
import { navigate, useRoute } from "./navigation.js";
import { messageOf, useResource } from "./resource.js";
import { Deferred } from "./shared.js";
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
  const discovery = useResource(route.page === "apps" ? "candidates" : null, discoverHosts, 10000);
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
  const connecting = !ready && snapshot.loading && !snapshot.data;
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
              <AppShell.NavigationToggle />
              <Heading size="body-large">Flint</Heading>
            </Flex>
          </AppShell.Header>
        ) : null}
        <AppShell.Navigation collapsible label="Flint navigation">
          <AppShell.NavGroup>
            <AppShell.NavItem
              active={route.page === "apps"}
              href="#/apps"
              icon={AppWindow}
              badge={instances.length}
            >
              Applications
            </AppShell.NavItem>
            <AppShell.NavItem
              active={route.page === "workflows"}
              href="#/workflows"
              icon={Layers}
            >
              Workflows
            </AppShell.NavItem>
          </AppShell.NavGroup>
          <AppShell.NavGroup placement="end">
            <AppShell.NavItem
              active={route.page === "settings" || route.page === "legal"}
              href="#/settings"
              icon={SettingsGlyph}
            >
              Settings
            </AppShell.NavItem>
          </AppShell.NavGroup>
        </AppShell.Navigation>
        {ready ? null : connecting ? (
          <Deferred delay={1000}>
            <AppShell.Banner>
              <Callout.Root tone="warning">
                <Callout.Icon>
                  <Icon glyph={LoaderCircle} size="sm" />
                </Callout.Icon>
                <Callout.Body>
                  <Callout.Text>Connecting to the backend…</Callout.Text>
                </Callout.Body>
              </Callout.Root>
            </AppShell.Banner>
          </Deferred>
        ) : (
          <AppShell.Banner>
            <Callout.Root tone="danger">
              <Callout.Icon>
                <Icon glyph={CircleAlert} size="sm" />
              </Callout.Icon>
              <Callout.Body>
                <Callout.Text>Cannot reach the backend: {snapshot.error}</Callout.Text>
              </Callout.Body>
              <Callout.Actions>
                <Callout.Action label="Try again" onSelect={snapshot.reload} priority="primary" />
              </Callout.Actions>
            </Callout.Root>
          </AppShell.Banner>
        )}
        <AppShell.Main>
          {route.page === "apps" ? (
            <Applications
              route={route}
              snapshot={snapshot.data}
              loading={snapshot.loading}
              refresh={snapshot.reload}
              discovery={discovery}
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
