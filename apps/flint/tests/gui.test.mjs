import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { _electron } from "playwright-core";
import { preview } from "vite";

const appRoot = dirname(dirname(fileURLToPath(import.meta.url)));

test("desktop navigation preserves connection identity, asynchronous selection and service actions", async () => {
  // Serve the window's own content security policy, so the view loads under the rules Tauri applies.
  const tauri = JSON.parse(await readFile(join(appRoot, "src-tauri/tauri.conf.json"), "utf8"));
  const server = await preview({
    preview: {
      host: "127.0.0.1",
      port: 0,
      headers: { "Content-Security-Policy": tauri.app.security.csp },
    },
  });
  const address = server.httpServer.address();
  assert.ok(address && typeof address !== "string");
  const application = await _electron.launch({
    args: [join(appRoot, "tests/harness.cjs")],
    cwd: appRoot,
  });
  try {
    const page = await application.firstWindow();
    await page.setViewportSize({ width: 1280, height: 850 });
    await page.addInitScript(() => {
      window.__cspViolations = [];
      document.addEventListener("securitypolicyviolation", (event) =>
        window.__cspViolations.push(`${event.effectiveDirective} ${event.blockedURI}`),
      );
    });
    await page.addInitScript(() => {
      window.__invokeCalls = [];
      window.__mockSnapshot = {
        backend: {
          ready: true,
          pid: 4312,
          bridge_address: "127.0.0.1",
          bridge_port: 6321,
        },
        instances: [],
      };
      const summary = (id, name) => ({
        workflow_id: id,
        name,
        description: "",
        execution_count: 1,
        instance_ids: ["maya-1"],
        running_count: 0,
        failed_count: 0,
        updated_at: "2026-09-29T10:00:00Z",
      });
      window.__workflows = [
        summary("first", "Asset check"),
        summary("second", "Material check"),
      ];
      const record = (id, name) => ({
        workflow_id: id,
        name,
        description: "",
        created_at: "2026-09-29T10:00:00Z",
        execs: [
          {
            execution_id: "0001",
            name: "Inspect scene",
            instance_id: "maya-1",
            code: `print('${name}')`,
            status: "succeeded",
            stdout: name,
            stderr: "",
            error: null,
            traceback: null,
            started_at: "2026-09-29T10:00:00Z",
            finished_at: "2026-09-29T10:00:01Z",
          },
        ],
      });
      window.__TAURI__ = {
        core: {
          invoke: async (command, args) => {
            window.__invokeCalls.push({ command, args });
            if (command === "activate_title_bar") return "custom";
            if (command === "snapshot")
              return structuredClone(window.__mockSnapshot);
            if (command === "candidates")
              return {
                hosts: [
                  {
                    host: "maya",
                    pid: 4520,
                    executable: "C:/Maya/maya.exe",
                  },
                ],
              };
            if (command === "workflows")
              return structuredClone(window.__workflows);
            if (command === "workflow") {
              if (args.id === "first" && window.__delayFirst)
                return new Promise((resolve) => {
                  window.__finishFirst = () =>
                    resolve(record("first", "Asset check"));
                });
              return record(
                args.id,
                args.id === "first" ? "Asset check" : "Material check",
              );
            }
            if (command === "host_info") {
              if (args.preview !== true)
                throw new Error("Application cards must request previews explicitly");
              window.__captures = (window.__captures ?? 0) + 1;
              window.__peakCaptures = Math.max(
                window.__peakCaptures ?? 0,
                window.__captures,
              );
              if (window.__captures > 2) {
                window.__captures -= 1;
                throw new Error("Window previews are busy");
              }
              await new Promise((resolve) => setTimeout(resolve, 80));
              window.__captures -= 1;
              if (args.pid === 4522)
                return {
                  pid: args.pid,
                  host: "maya",
                  executable: "C:/Maya/maya.exe",
                  window: { title: "Scene maya-3", minimized: true },
                  preview: { image: null, unavailable_reason: "Window is minimized" },
                };
              return {
                pid: args.pid,
                host: "maya",
                executable: "C:/Maya/maya.exe",
                window: {
                  minimized: false,
                  title: args.pid === 4520
                    ? "Character_Rig.ma"
                    : `Scene maya-${args.pid - 4519}`,
                },
                preview: {
                  image:
                    "data:image/svg+xml," +
                    encodeURIComponent(
                      '<svg xmlns="http://www.w3.org/2000/svg" width="640" height="400"><rect width="640" height="400" fill="#344842"/><rect x="16" y="40" width="150" height="344" fill="#263630"/><rect x="182" y="40" width="442" height="344" fill="#78988b"/><text x="208" y="214" font-size="24" fill="white">Character_Rig.ma</text></svg>',
                    ),
                  unavailable_reason: null,
                },
              };
            }
            if (command === "focus_application") return;
            if (command === "attach")
              return { attached: true, pid: args.pid, host: args.hostKind, instance_id: "maya-1", execution_ready: true };
            if (command === "desktop_info")
              return {
                version: "0.1.0",
                state_dir: "C:/FlintData",
                control_endpoint: "127.0.0.1:6322",
                bridge_endpoint: "127.0.0.1:6321",
                attach_supported: true,
              };
            if (command === "stop_backend") {
              if (!window.__allowStop)
                throw new Error("Executions are still active");
              return;
            }
            throw new Error(`Unexpected command: ${command}`);
          },
        },
      };
    });
    const url = `http://127.0.0.1:${address.port}/`;
    await page.goto(url);
    await page
      .getByText("No connected applications", { exact: true })
      .waitFor();
    await page.getByRole("article").getByRole("link").click();
    await page.getByText("C:/Maya/maya.exe", { exact: true }).waitFor();
    await page.getByRole("button", { name: "Attach Bridge", exact: true }).last().click();
    assert.deepEqual(
      await page.evaluate(() => window.__invokeCalls.find((call) => call.command === "attach").args),
      { pid: 4520, hostKind: "maya" },
    );

    // A discovered process becomes connected while its detail is open. The same PID must not
    // remain in both sections, and its window operations retain the process identity.
    await page.evaluate(() => {
      window.__mockSnapshot.instances = [
        {
          instance_id: "maya-1",
          instance_name: "Maya session",
          instance_type: "maya",
          pid: 4520,
          runtime_version: "Python 3.11",
          bridge_version: "0.1.0",
          execution_ready: true,
        },
      ];
    });
    await page.getByText("Ready to execute", { exact: true }).waitFor();
    await page
      .getByRole("button", { name: "Switch to application", exact: true })
      .click();
    await page.waitForFunction(() => {
      const image = document.querySelector(
        'img[alt="Window preview of Character_Rig.ma"]',
      );
      return image?.complete && image.naturalWidth === 640;
    });
    assert.ok(
      await page.evaluate(() =>
        window.__invokeCalls.some(
          (call) =>
            call.command === "focus_application" &&
            call.args.pid === 4520 && Object.keys(call.args).length === 1,
        ),
      ),
    );
    await page.getByRole("button", { name: "Back to applications" }).click();
    await page
      .getByRole("link", { name: "Character_Rig.ma", exact: true })
      .waitFor();
    assert.equal(
      await page.getByRole("link", { name: "Maya", exact: true }).count(),
      0,
    );
    // The thumbnail belongs to the card's navigation target, not just its text link.
    const card = page.getByRole("article").filter({
      has: page.getByRole("link", { name: "Character_Rig.ma", exact: true }),
    });
    const bounds = await card.boundingBox();
    assert.ok(bounds);
    await page.mouse.click(bounds.x + bounds.width / 2, bounds.y + 50);
    await page
      .getByRole("button", { name: "Switch to application", exact: true })
      .waitFor();
    assert.equal(new URL(page.url()).hash, "#/apps/maya-1");

    // More cards than capture slots must all receive previews without flooding native workers.
    await page.evaluate(() => {
      const instance = window.__mockSnapshot.instances[0];
      window.__mockSnapshot.instances.push(
        { ...instance, instance_id: "maya-2", pid: 4521 },
        { ...instance, instance_id: "maya-3", pid: 4522 },
      );
    });
    await page.getByRole("button", { name: "Back to applications" }).click();
    await page
      .getByRole("link", { name: "Scene maya-2", exact: true })
      .waitFor();
    await page
      .getByRole("link", { name: "Scene maya-3", exact: true })
      .waitFor();
    assert.equal(await page.evaluate(() => window.__peakCaptures), 2);
    await page.getByRole("article").filter({
      has: page.getByRole("link", { name: "Scene maya-3", exact: true }),
    }).getByText("Window is minimized", { exact: true }).waitFor();

    // Connected registrations are not restricted to the built-in discovery kinds.
    await page.evaluate(() => {
      window.__mockSnapshot.instances.push({
        ...window.__mockSnapshot.instances[0],
        instance_id: "custom-1",
        instance_type: "custom-editor",
        pid: 4523,
      });
    });
    await page.getByText("custom-editor · PID 4523", { exact: true }).waitFor();

    await page.getByRole("link", { name: "Workflows", exact: true }).click();
    await page.evaluate(() => {
      window.__delayFirst = true;
    });
    await page.getByRole("button", { name: /Asset check/ }).click();
    await page.waitForFunction(
      () => typeof window.__finishFirst === "function",
    );
    await page.getByRole("button", { name: /Material check/ }).click();
    await page
      .getByRole("heading", { name: "Material check", exact: true })
      .waitFor();
    await page.evaluate(() => {
      window.__finishFirst();
    });
    await page.getByRole("link", { name: /Inspect scene/ }).click();
    assert.equal(new URL(page.url()).hash, "#/workflows/second/executions/0001");
    assert.equal(
      await page.getByLabel("Output", { exact: true }).textContent(),
      "Material check",
    );
    assert.equal(
      await page.getByLabel("Code", { exact: true }).textContent(),
      "print('Material check')",
    );

    await page.setViewportSize({ width: 500, height: 750 });
    await page
      .getByRole("button", { name: "Back to workflow", exact: true })
      .click();
    await page.getByRole("link", { name: /Inspect scene/ }).waitFor();
    await page
      .getByRole("button", { name: "Back to Workflows", exact: true })
      .click();
    await page.getByRole("button", { name: /Material check/ }).waitFor();
    assert.equal(new URL(page.url()).hash, "#/workflows");
    await page.setViewportSize({ width: 1280, height: 850 });
    await page.getByRole("link", { name: "Settings", exact: true }).click();
    await page.getByRole("combobox", { name: "Theme" }).click();
    await page.getByRole("option", { name: "Dark", exact: true }).click();
    await page.reload();
    await page.waitForFunction(
      () => document.documentElement.dataset.cairnAppearance === "dark",
    );
    await page.getByRole("link", { name: "Open-source licenses" }).click();
    await page.getByRole("heading", { name: "Typography licenses" }).waitFor();
    await page.getByRole("button", { name: "Back to settings" }).click();

    await page
      .getByRole("button", { name: "Stop", exact: true })
      .click();
    await page
      .getByRole("alertdialog")
      .getByRole("button", { name: "Stop backend", exact: true })
      .click();
    await page
      .getByText("Executions are still active", { exact: true })
      .waitFor();
    await page.getByRole("alertdialog").waitFor({ state: "hidden" });
    await page.evaluate(() => {
      window.__allowStop = true;
    });
    await page
      .getByRole("button", { name: "Stop", exact: true })
      .click();
    await page
      .getByRole("alertdialog")
      .getByRole("button", { name: "Stop backend", exact: true })
      .click();
    assert.equal(
      await page.evaluate(
        () =>
          window.__invokeCalls.filter((call) => call.command === "stop_backend")
            .length,
      ),
      2,
    );
    assert.deepEqual(await page.evaluate(() => window.__cspViolations), []);
  } finally {
    await application.close();
    await new Promise((resolve, reject) =>
      server.httpServer.close((error) => (error ? reject(error) : resolve())),
    );
  }
});
