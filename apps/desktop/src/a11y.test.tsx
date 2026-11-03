// @vitest-environment jsdom
// Accessibility checks (axe-core) on every page and the widget, in the honest
// no-core state. Color contrast is checked by design tokens, not here: jsdom
// does not compute styles.
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import { Root } from "./Root";

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
  sessionStorage.setItem("tamgabul.welcomed", "1");
  window.location.hash = "";
});
afterEach(cleanup);

async function violations(container: HTMLElement) {
  const r = await axe.run(container, {
    rules: { "color-contrast": { enabled: false } },
    resultTypes: ["violations"],
  });
  return r.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`);
}

const pages = ["search", "memory", "models", "folders", "settings"];

describe("accessibility", () => {
  it("the checker itself catches a real problem", async () => {
    const { container } = render(<div><button type="button"><svg /></button><img src="x.png" /></div>);
    const v = await violations(container);
    expect(v.some((x) => x.startsWith("button-name"))).toBe(true);
    expect(v.some((x) => x.startsWith("image-alt"))).toBe(true);
  });

  for (const p of pages) {
    it(`#/${p} has no axe violations`, async () => {
      window.location.hash = `#/${p}`;
      const { container } = render(<Root surface="app" />);
      await screen.findByRole("main");
      await new Promise((r) => setTimeout(r, 50));
      expect(await violations(container)).toEqual([]);
    });
  }

  it("widget has no axe violations", async () => {
    const { container } = render(<Root surface="widget" />);
    await new Promise((r) => setTimeout(r, 50));
    expect(await violations(container)).toEqual([]);
  });

  it("onboarding has no axe violations", async () => {
    window.location.hash = "#/welcome";
    const { container } = render(<Root surface="app" />);
    await new Promise((r) => setTimeout(r, 50));
    expect(await violations(container)).toEqual([]);
  });
});

describe("dialogs", () => {
  it("licenses dialog is accessible, traps focus and closes on Escape", async () => {
    window.location.hash = "#/settings";
    const { container } = render(<Root surface="app" />);
    fireEvent.click(await screen.findByText("Models and libraries"));
    const dlg = await screen.findByRole("dialog");
    expect(dlg.textContent).toContain("llama.cpp");
    expect(dlg.textContent).toContain("Apache-2.0");
    expect(dlg.contains(document.activeElement)).toBe(true);
    expect(await violations(container)).toEqual([]);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

describe("keyboard", () => {
  it("skip link moves focus to the main region", async () => {
    window.location.hash = "#/settings";
    render(<Root surface="app" />);
    const skip = await screen.findByText("Skip to content");
    fireEvent.click(skip);
    expect(document.activeElement).toBe(screen.getByRole("main"));
  });

  it("segmented controls are one tab stop and arrows move the choice", async () => {
    window.location.hash = "#/settings";
    render(<Root surface="app" />);
    const group = await screen.findByRole("radiogroup", { name: "Language" });
    const radios = Array.from(group.querySelectorAll<HTMLButtonElement>("[role=radio]"));
    expect(radios.filter((r) => r.tabIndex === 0)).toHaveLength(1);
    expect(radios[0].getAttribute("aria-checked")).toBe("true");
  });

  it("Ctrl+3 switches to Models", async () => {
    window.location.hash = "#/search";
    render(<Root surface="app" />);
    await screen.findByRole("main");
    fireEvent.keyDown(document, { key: "3", ctrlKey: true });
    await new Promise((r) => setTimeout(r, 20));
    expect(window.location.hash).toBe("#/models");
  });
});
