// Shared axe-core accessibility check for the demo and host Playwright suites
// (M6-T10). Every page is expected to be free of WCAG 2.0/2.1/2.2 A and AA
// violations plus axe's best-practice rules; failures print the offending
// selectors so CI logs point straight at the element.

import AxeBuilder from "@axe-core/playwright";
import { expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import type { AxeResults } from "axe-core";

/** Rule tags enforced on every page. */
export const AXE_TAGS = [
  "wcag2a",
  "wcag2aa",
  "wcag21a",
  "wcag21aa",
  "wcag22aa",
  "best-practice",
];

type Violation = AxeResults["violations"][number];

function describe(violation: Violation): string {
  const nodes = violation.nodes.map((node) => {
    const target = node.target.join(" ");
    const summary = (node.failureSummary ?? "").replace(/\s+/g, " ").trim();
    return `  ${target}: ${summary}`;
  });
  return `${violation.id} (${violation.impact ?? "unknown"}) — ${violation.help}\n${nodes.join("\n")}`;
}

/** Run axe over the whole page, or one CSS scope, and return readable failures. */
export async function axeViolations(
  page: Page,
  scope?: string,
): Promise<string[]> {
  let builder = new AxeBuilder({ page }).withTags(AXE_TAGS);
  if (scope) builder = builder.include(scope);
  const results = await builder.analyze();
  return results.violations.map(describe);
}

/** Assert that the page has no axe violations. */
export async function expectNoViolations(
  page: Page,
  scope?: string,
): Promise<void> {
  const violations = await axeViolations(page, scope);
  expect(violations, violations.join("\n\n")).toEqual([]);
}
