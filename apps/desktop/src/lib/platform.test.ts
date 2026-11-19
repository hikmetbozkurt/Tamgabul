import { describe, expect, it } from "vitest";
import { isChildPath, isMacOs, metaKeyLabel, modKeyLabel, pathPlaceholder } from "./platform";

describe("platform helpers", () => {
  it("distinguishes macOS labels and placeholders", () => {
    expect(isMacOs("macos")).toBe(true);
    expect(isMacOs("windows")).toBe(false);
    expect(modKeyLabel("macos")).toBe("Cmd");
    expect(modKeyLabel("windows")).toBe("Ctrl");
    expect(metaKeyLabel("macos")).toBe("Cmd");
    expect(metaKeyLabel("windows")).toBe("Win");
    expect(pathPlaceholder("folder", "macos")).toBe("~/Documents");
    expect(pathPlaceholder("folder", "windows")).toBe("C:\\Projects");
    expect(pathPlaceholder("model", "macos")).toBe("~/Models/…");
  });

  it("checks child paths across Windows backslashes, drive roots, and POSIX", () => {
    expect(isChildPath("C:\\Projects\\sub", "C:\\Projects")).toBe(true);
    expect(isChildPath("C:/Projects/sub", "C:\\Projects\\")).toBe(true);
    expect(isChildPath("c:\\projects\\sub", "C:\\Projects")).toBe(true);
    expect(isChildPath("C:\\Projects\\sub", "C:\\")).toBe(true);
    expect(isChildPath("C:\\Projects2", "C:\\Projects")).toBe(false);
    expect(isChildPath("C:\\Projects", "C:\\Projects")).toBe(false);
    expect(isChildPath("D:\\Projects", "C:\\")).toBe(false);

    expect(isChildPath("\\\\NAS\\Docs\\2026", "\\\\nas\\docs")).toBe(true);
    expect(isChildPath("\\\\nas\\docs2\\2026", "\\\\nas\\docs")).toBe(false);
    expect(isChildPath("\\\\nas\\docs", "\\\\nas\\docs")).toBe(false);

    expect(isChildPath("/Users/a/docs", "/Users/a")).toBe(true);
    expect(isChildPath("/Users/a/docs", "/")).toBe(true);
    expect(isChildPath("/Users/ab", "/Users/a")).toBe(false);
    expect(isChildPath("/Users/a", "/Users/a")).toBe(false);
  });
});
