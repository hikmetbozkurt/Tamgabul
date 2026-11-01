// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Root } from "./Root";
import { comboFromEvent } from "./features/settings/SettingsPage";

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
  window.location.hash = "";
});
afterEach(cleanup);

const renderApp = (hash = "", settings?: object) => {
  if (settings) localStorage.setItem("tamgabul.settings", JSON.stringify(settings));
  window.location.hash = hash;
  return render(<Root surface="app" />);
};

describe("app surface (demo data)", () => {
  it("shows the search home in English with the MOCK badge", async () => {
    renderApp("#/search");
    expect(await screen.findByRole("heading", { name: "Tamgabul", level: 1 })).toBeTruthy();
    expect(screen.getAllByText("MOCK ONLY").length).toBeGreaterThan(0);
    expect(await screen.findByText("System Status")).toBeTruthy();
    expect(screen.getByRole("link", { name: /Search/ }).getAttribute("aria-current")).toBe("page");
  });

  it("searching shows a result card with Preview and Open", async () => {
    renderApp("#/search");
    const input = await screen.findByPlaceholderText("Ask about a file, note or past conversation...");
    await waitFor(() => expect((input as HTMLInputElement).disabled).toBe(false));
    fireEvent.change(input, { target: { value: "auction timer" } });
    fireEvent.submit(input.closest("form")!);
    expect(await screen.findByText("auction-timer-bug.md", {}, { timeout: 3000 })).toBeTruthy();
    expect(screen.getAllByRole("button", { name: /Preview/ }).length).toBeGreaterThan(0);
  });

  it("switches to Turkish from settings", async () => {
    renderApp("#/settings");
    const tr = await screen.findByRole("radio", { name: /Türkçe/ });
    fireEvent.click(tr);
    expect(await screen.findByRole("heading", { name: "Ayarlar" })).toBeTruthy();
    expect(screen.getByRole("link", { name: /Klasörler/ })).toBeTruthy();
  });

  it.each(["#/memory", "#/models", "#/folders"])("renders %s without crashing", async (hash) => {
    renderApp(hash);
    expect(await screen.findByRole("heading", { level: 1 })).toBeTruthy();
  });

  it("first run opens onboarding with EmbeddingGemma 2 required", async () => {
    renderApp("#/search", { demo_data: true, demo_scenario: "first_run" });
    expect(await screen.findByText("Welcome to Tamgabul")).toBeTruthy();
    expect(screen.getAllByText("Required · not installed").length).toBeGreaterThan(0);
  });

  it("real mode (no core in browser) shows honest states, not demo data", async () => {
    renderApp("#/folders", { demo_data: false });
    expect(await screen.findByText("Not available yet")).toBeTruthy();
    expect(screen.queryByText("MOCK ONLY")).toBeNull();
  });
});

describe("widget surface", () => {
  it("renders compact search and footer", async () => {
    render(<Root surface="widget" />);
    expect(await screen.findByText("Search ready")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Open full app" })).toBeTruthy();
  });
});

describe("hotkey capture", () => {
  it("needs a modifier and maps space", () => {
    const base = { ctrlKey: false, shiftKey: false, altKey: false, metaKey: false };
    expect(comboFromEvent({ ...base, key: "k" })).toBeNull();
    expect(comboFromEvent({ ...base, key: "Control", ctrlKey: true })).toBeNull();
    expect(comboFromEvent({ ...base, key: " ", ctrlKey: true, shiftKey: true })).toBe("Ctrl+Shift+Space");
    expect(comboFromEvent({ ...base, key: "j", altKey: true })).toBe("Alt+J");
  });
});

// keep act imported for React 18 strict-mode warnings in some environments
void act;
