import { fireEvent, render, screen, within } from "@testing-library/react";
import { ThemeProvider } from "styled-components";
import { MemoryRouter } from "react-router-dom";

import { FleetMembers } from "./Fleet";
import { AuthContext } from "../../contexts";
import theme from "../../App/theme";
import { apiCall } from "../../api";

jest.mock("../../api", () => ({
  apiCall: jest.fn(),
  useApi: jest.fn(() => [null, jest.fn()]),
  errorToaster: jest.fn(),
  toaster: jest.fn(),
}));

const FC_ID = 1001;

// A character owned by a player who has authenticated with the site.
function linked(id, name, accountId, accountName, shipName) {
  return {
    id,
    name,
    account_id: accountId,
    account_name: accountName,
    ship: { id: 28659, name: shipName },
    wl_category: null,
  };
}

// A character that has never authenticated, so it cannot be attributed to a player.
function unlinked(id, name, shipName) {
  return linked(id, name, null, null, shipName);
}

async function renderWithMembers(members) {
  apiCall.mockResolvedValue({ members });
  const utils = render(
    <ThemeProvider theme={theme.Light}>
      <MemoryRouter>
        <AuthContext.Provider value={{ current: { id: FC_ID }, access: {} }}>
          <FleetMembers refreshedAt={0} />
        </AuthContext.Provider>
      </MemoryRouter>
    </ThemeProvider>
  );
  // wait for the async fetch to resolve and the section to appear
  await screen.findByText(/^Multiboxers \(\d+\)$/);
  return utils;
}

// The multiboxer cards, in page order. Each card is labelled with its player.
function cards() {
  const list = screen.getByRole("list", { name: "Multiboxers" });
  return within(list)
    .getAllByRole("listitem")
    .map((card) => {
      const countPill = within(card).getByText(/characters$/);
      return {
        player: card.getAttribute("aria-label"),
        count: Number(countPill.textContent.match(/\d+/)[0]),
        countPill,
        text: card.textContent,
      };
    });
}

function expectNoMultiboxers() {
  expect(screen.queryByRole("list", { name: "Multiboxers" })).not.toBeInTheDocument();
  expect(screen.getByText("Nobody is flying more than one character.")).toBeInTheDocument();
}

// Opens the collapsed list of single-character pilots and returns its rows' text
function expandEveryoneElse() {
  fireEvent.click(screen.getByRole("button", { name: /pilots on one character/ }));
  const list = screen.getByRole("list", { name: "Everyone else" });
  return within(list)
    .getAllByRole("listitem")
    .map((row) => row.textContent);
}

function multiboxingCount() {
  return Number(screen.getByText(/^Multiboxers \(\d+\)$/).textContent.match(/\d+/)[0]);
}

afterEach(() => {
  jest.clearAllMocks();
});

describe("Characters per player", () => {
  test("a fleet of single-character pilots has no multiboxers and lists everyone else", async () => {
    const { container } = await renderWithMembers([
      linked(1, "Alpha Pilot", 1, "Alpha Pilot", "Paladin"),
      linked(2, "Bravo Pilot", 2, "Bravo Pilot", "Nestor"),
      linked(3, "Charlie Pilot", 3, "Charlie Pilot", "Vargur"),
    ]);

    expectNoMultiboxers();
    expect(multiboxingCount()).toBe(0);
    expect(container.textContent).toMatch(/3 characters · 3 pilots/);
    // nobody is unattributed, so it isn't mentioned at all
    expect(container.textContent).not.toMatch(/unlinked/);
    expect(expandEveryoneElse()).toHaveLength(3);
  });

  test("a player flying multiple characters is collapsed into one card with the right count", async () => {
    const { container } = await renderWithMembers([
      linked(10, "Multi Main", 10, "Multi Main", "Paladin"),
      linked(11, "Multi Alt A", 10, "Multi Main", "Kronos"),
      linked(12, "Multi Alt B", 10, "Multi Main", "Nestor"),
      linked(20, "Solo Pilot", 20, "Solo Pilot", "Vargur"),
    ]);

    expect(cards()).toHaveLength(1);
    expect(cards()[0]).toMatchObject({ player: "Multi Main", count: 3 });
    // 4 characters in fleet, but only 2 actual humans
    expect(container.textContent).toMatch(/4 characters · 2 pilots/);
    expect(multiboxingCount()).toBe(1);
    const rest = expandEveryoneElse();
    expect(rest).toHaveLength(1);
    expect(rest[0]).toMatch(/Solo Pilot/);
  });

  test("each character is listed with its ship so the FC knows what can be dropped", async () => {
    await renderWithMembers([
      linked(10, "Multi Main", 10, "Multi Main", "Paladin"),
      linked(11, "Multi Alt A", 10, "Multi Main", "Kronos"),
    ]);

    const { text } = cards()[0];
    // the player's own character is shown as "main"
    expect(text).toMatch(/Paladin\s*main/);
    expect(text).toMatch(/Kronos\s*Multi Alt A/);
  });

  test("cards are sorted by character count, highest first", async () => {
    await renderWithMembers([
      linked(1, "One Char", 1, "One Char", "Nestor"),
      linked(30, "Three Chars", 30, "Three Chars", "Paladin"),
      linked(31, "Three Alt A", 30, "Three Chars", "Kronos"),
      linked(32, "Three Alt B", 30, "Three Chars", "Vargur"),
      linked(20, "Two Chars", 20, "Two Chars", "Paladin"),
      linked(21, "Two Alt A", 20, "Two Chars", "Nestor"),
    ]);

    expect(cards().map((c) => c.count)).toEqual([3, 2]);
    expect(cards().map((c) => c.player)).toEqual(["Three Chars", "Two Chars"]);
    expect(expandEveryoneElse()[0]).toMatch(/One Char/);
  });

  test("players with an equal count are ordered by name so the list is stable", async () => {
    await renderWithMembers([
      linked(20, "Zulu Player", 20, "Zulu Player", "Paladin"),
      linked(21, "Zulu Alt", 20, "Zulu Player", "Nestor"),
      linked(10, "Alpha Player", 10, "Alpha Player", "Kronos"),
      linked(11, "Alpha Alt", 10, "Alpha Player", "Vargur"),
    ]);

    expect(cards().map((c) => c.player)).toEqual(["Alpha Player", "Zulu Player"]);
  });

  test("unlinked characters get their own row each and are never merged into one fake player", async () => {
    const { container } = await renderWithMembers([
      linked(10, "Real Player", 10, "Real Player", "Paladin"),
      unlinked(50, "Ghost One", "Vindicator"),
      unlinked(51, "Ghost Two", "Nestor"),
    ]);

    // 3 rows, not 2 - the two unattributed characters must not collapse together
    expectNoMultiboxers();
    const rest = expandEveryoneElse();
    expect(rest).toHaveLength(3);
    expect(rest.filter((row) => /unlinked/.test(row))).toHaveLength(2);
    expect(rest.some((row) => /Ghost One/.test(row))).toBe(true);
    expect(rest.some((row) => /Ghost Two/.test(row))).toBe(true);
    // only the attributable character counts as a known pilot
    expect(container.textContent).toMatch(/3 characters · 1 pilots? · 2 unlinked/);
  });

  test("the multiboxing tally counts players with 2+ characters, not the characters themselves", async () => {
    const { container } = await renderWithMembers([
      linked(10, "Boxer One", 10, "Boxer One", "Paladin"),
      linked(11, "Boxer One Alt", 10, "Boxer One", "Kronos"),
      linked(20, "Boxer Two", 20, "Boxer Two", "Nestor"),
      linked(21, "Boxer Two Alt", 20, "Boxer Two", "Vargur"),
      linked(30, "Lone Pilot", 30, "Lone Pilot", "Paladin"),
    ]);

    // 2 multiboxers (not 4 characters, and not 3 pilots)
    expect(multiboxingCount()).toBe(2);
    expect(container.textContent).toMatch(/5 characters · 3 pilots/);
  });

  test("an empty fleet renders without crashing", async () => {
    const { container } = await renderWithMembers([]);

    expectNoMultiboxers();
    expect(multiboxingCount()).toBe(0);
    expect(container.textContent).toMatch(/0 characters · 0 pilots/);
    // nobody to list, so no button to expand
    expect(
      screen.queryByRole("button", { name: /pilots on one character/ })
    ).not.toBeInTheDocument();
  });

  // In the backend, `name` and `account_id` fall out of the same match, so a null
  // name implies a null account_id - such a character always takes the unlinked path.
  test("a character with no resolved name falls back to Unknown on the unlinked path", async () => {
    await renderWithMembers([unlinked(99, null, "Paladin")]);

    const rest = expandEveryoneElse();
    expect(rest).toHaveLength(1);
    expect(rest[0]).toMatch(/Unknown/);
    expect(rest[0]).toMatch(/unlinked/);
  });

  // account_name is looked up separately from account_id, so it can be missing on a
  // character that IS attributable. Without a fallback the card's title renders blank.
  test("a linked player with no resolved account name still identifies someone", async () => {
    await renderWithMembers([
      linked(10, "Visible Character", 10, null, "Paladin"),
      linked(11, "Second Character", 10, null, "Kronos"),
    ]);

    expect(cards()).toHaveLength(1);
    expect(cards()[0]).toMatchObject({ player: "Visible Character", count: 2 });
  });

  test("only heavy multiboxers get their count emphasised", async () => {
    await renderWithMembers([
      linked(10, "Heavy Boxer", 10, "Heavy Boxer", "Paladin"),
      linked(11, "Heavy Alt A", 10, "Heavy Boxer", "Kronos"),
      linked(12, "Heavy Alt B", 10, "Heavy Boxer", "Nestor"),
      linked(20, "Light Boxer", 20, "Light Boxer", "Vargur"),
      linked(21, "Light Alt", 20, "Light Boxer", "Paladin"),
    ]);

    const [heavy, light] = cards();
    expect(heavy).toMatchObject({ player: "Heavy Boxer", count: 3 });
    expect(light).toMatchObject({ player: "Light Boxer", count: 2 });
    // assert the difference, not the exact styling
    expect(heavy.countPill.className).not.toEqual(light.countPill.className);
  });
});

describe("the rest of the fleet page still renders", () => {
  test("the fleet composition chips count ship types", async () => {
    const { container } = await renderWithMembers([
      linked(1, "Pally Pilot", 1, "Pally Pilot", "Paladin"),
      linked(2, "Kron Pilot", 2, "Kron Pilot", "Kronos"),
      linked(3, "Logi Pilot", 3, "Logi Pilot", "Nestor"),
    ]);

    expect(container.textContent).toMatch(/2\s*Marauders/);
    expect(container.textContent).toMatch(/1\s*Logistics/);
    expect(container.textContent).toMatch(/0\s*Vindicators/);
  });

  test("every character has a link to its skills", async () => {
    await renderWithMembers([
      linked(10, "Multi Main", 10, "Multi Main", "Paladin"),
      linked(11, "Multi Alt A", 10, "Multi Main", "Kronos"),
      linked(20, "Solo Pilot", 20, "Solo Pilot", "Vargur"),
    ]);

    expandEveryoneElse();
    expect(screen.getAllByRole("link", { name: "Skills" })).toHaveLength(3);
  });
});
