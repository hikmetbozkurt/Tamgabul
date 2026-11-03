// @vitest-environment jsdom
// Render tests without the Rust core attached: the UI must show honest states
// (setup needed, not available yet) and never invent data.
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Root } from "./Root";
import { comboFromEvent } from "./features/settings/SettingsPage";

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
  window.location.hash = "";
});
afterEach(cleanup);

const renderApp = (hash = "") => {
  window.location.hash = hash;
  return render(<Root surface="app" />);
};

describe("app surface without a model", () => {
  it("first launch opens onboarding with EmbeddingGemma 2 required", async () => {
    renderApp("#/search");
    expect(await screen.findByText("Welcome to Tamgabul")).toBeTruthy();
    expect(screen.getAllByText("Required · not installed").length).toBeGreaterThan(0);
    expect((screen.getByRole("button", { name: "Start searching" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("search is disabled and explains why", async () => {
    sessionStorage.setItem("tamgabul.welcomed", "1");
    renderApp("#/search");
    const input = await screen.findByPlaceholderText("Semantic search needs EmbeddingGemma 2");
    expect((input as HTMLInputElement).disabled).toBe(true);
    expect(screen.queryByText("Recent searches")).toBeNull();
  });

  it("models page shows four empty slots, nothing invented", async () => {
    sessionStorage.setItem("tamgabul.welcomed", "1");
    renderApp("#/models");
    expect(await screen.findByText("Not installed. Search needs it.")).toBeTruthy();
    expect(screen.getAllByText("Not installed").length).toBeGreaterThanOrEqual(3);
  });

  it("#/memory shows the real (empty) history", async () => {
    sessionStorage.setItem("tamgabul.welcomed", "1");
    renderApp("#/memory");
    expect(await screen.findByText("No history yet")).toBeTruthy();
  });

  it("#/folders shows the real (empty) folder list", async () => {
    sessionStorage.setItem("tamgabul.welcomed", "1");
    renderApp("#/folders");
    expect(await screen.findByText("No folders yet")).toBeTruthy();
  });

  it("switches to Turkish from settings", async () => {
    sessionStorage.setItem("tamgabul.welcomed", "1");
    renderApp("#/settings");
    fireEvent.click(await screen.findByRole("radio", { name: /Türkçe/ }));
    expect(await screen.findByRole("heading", { name: "Ayarlar" })).toBeTruthy();
    expect(screen.getByRole("link", { name: /Klasörler/ })).toBeTruthy();
  });

  it("settings has no demo or developer section", async () => {
    sessionStorage.setItem("tamgabul.welcomed", "1");
    renderApp("#/settings");
    await screen.findByRole("heading", { name: "Settings" });
    expect(screen.queryByText(/demo/i)).toBeNull();
    expect(screen.queryByText(/mock/i)).toBeNull();
  });
});

describe("widget surface", () => {
  it("shows setup needed when no model is installed", async () => {
    render(<Root surface="widget" />);
    expect(await screen.findByText("Setup needed")).toBeTruthy();
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

describe("understood line", () => {
  it("shows filters, exclusions and LLM-added words", async () => {
    const { I18nProvider } = await import("./i18n");
    const { UnderstoodLine } = await import("./features/search/Thread");
    render(
      <I18nProvider lang="en">
        <UnderstoodLine u={{ file_type: "pdf", date: "last_week", exclude: ["menu"], added_terms: ["invoice"], by_llm: true }} />
      </I18nProvider>,
    );
    const line = screen.getByTestId("understood");
    expect(line.textContent).toContain("PDF");
    expect(line.textContent).toContain("Last week");
    expect(line.textContent).toContain("without “menu”");
    expect(line.textContent).toContain("also searched: invoice");
  });

  it("renders nothing when there is nothing to show", async () => {
    const { I18nProvider } = await import("./i18n");
    const { UnderstoodLine } = await import("./features/search/Thread");
    const { container } = render(
      <I18nProvider lang="en">
        <UnderstoodLine u={{ file_type: "any", date: "any", exclude: [], added_terms: [], by_llm: false }} />
      </I18nProvider>,
    );
    expect(container.textContent).toBe("");
  });
});
