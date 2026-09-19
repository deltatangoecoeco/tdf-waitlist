import { render, screen, within } from "@testing-library/react";
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
  await screen.findByRole("heading", { name: /characters per player/i });
  return utils;
}

function playerRows() {
  const table = screen.getByRole("table", { name: /characters per player/i });
  const rows = within(table).getAllByRole("row").slice(1); // drop the header row
  return rows.map((row) => {
    const cells = within(row).getAllByRole("cell");
    return {
      player: cells[0].textContent.trim(),
      count: Number(cells[1].textContent.trim()),
      characters: cells[2].textContent.trim(),
      // the count is emphasised for multiboxers; assert the semantics, not the styling
      emphasised: cells[1].querySelector("strong") !== null,
    };
  });
}

afterEach(() => {
  jest.clearAllMocks();
});

describe("Characters per player", () => {
  test("a fleet of single-character pilots shows one row each, all counted as 1", async () => {
    const { container } = await renderWithMembers([
      linked(1, "Alpha Pilot", 1, "Alpha Pilot", "Paladin"),
      linked(2, "Bravo Pilot", 2, "Bravo Pilot", "Nestor"),
      linked(3, "Charlie Pilot", 3, "Charlie Pilot", "Vargur"),
    ]);

    const rows = playerRows();
    expect(rows).toHaveLength(3);
    expect(rows.every((r) => r.count === 1)).toBe(true);

    expect(container.textContent).toMatch(/Pilots:\s*3\b/);
    expect(container.textContent).toMatch(/Characters:\s*3\b/);
    expect(container.textContent).toMatch(/Multiboxing:\s*0\b/);
    // nobody is unattributed, so that box should be hidden entirely
    expect(container.textContent).not.toMatch(/Unlinked:/);
  });

  test("a player flying multiple characters is collapsed into one row with the right count", async () => {
    const { container } = await renderWithMembers([
      linked(10, "Multi Main", 10, "Multi Main", "Paladin"),
      linked(11, "Multi Alt A", 10, "Multi Main", "Kronos"),
      linked(12, "Multi Alt B", 10, "Multi Main", "Nestor"),
      linked(20, "Solo Pilot", 20, "Solo Pilot", "Vargur"),
    ]);

    const rows = playerRows();
    expect(rows).toHaveLength(2);

    expect(rows[0].player).toBe("Multi Main");
    expect(rows[0].count).toBe(3);
    expect(rows[1].player).toBe("Solo Pilot");
    expect(rows[1].count).toBe(1);

    // 4 characters in fleet, but only 2 actual humans
    expect(container.textContent).toMatch(/Pilots:\s*2\b/);
    expect(container.textContent).toMatch(/Characters:\s*4\b/);
    expect(container.textContent).toMatch(/Multiboxing:\s*1\b/);
  });

  test("each character is listed with its ship so the FC knows what can be dropped", async () => {
    await renderWithMembers([
      linked(10, "Multi Main", 10, "Multi Main", "Paladin"),
      linked(11, "Multi Alt A", 10, "Multi Main", "Kronos"),
    ]);

    const rows = playerRows();
    expect(rows[0].characters).toBe("Multi Main (Paladin), Multi Alt A (Kronos)");
  });

  test("rows are sorted by character count, highest first", async () => {
    await renderWithMembers([
      linked(1, "One Char", 1, "One Char", "Nestor"),
      linked(30, "Three Chars", 30, "Three Chars", "Paladin"),
      linked(31, "Three Alt A", 30, "Three Chars", "Kronos"),
      linked(32, "Three Alt B", 30, "Three Chars", "Vargur"),
      linked(20, "Two Chars", 20, "Two Chars", "Paladin"),
      linked(21, "Two Alt A", 20, "Two Chars", "Nestor"),
    ]);

    expect(playerRows().map((r) => r.count)).toEqual([3, 2, 1]);
    expect(playerRows().map((r) => r.player)).toEqual(["Three Chars", "Two Chars", "One Char"]);
  });

  test("players with an equal count are ordered by name so the list is stable", async () => {
    await renderWithMembers([
      linked(20, "Zulu Player", 20, "Zulu Player", "Paladin"),
      linked(21, "Zulu Alt", 20, "Zulu Player", "Nestor"),
      linked(10, "Alpha Player", 10, "Alpha Player", "Kronos"),
      linked(11, "Alpha Alt", 10, "Alpha Player", "Vargur"),
    ]);

    const rows = playerRows();
    expect(rows.map((r) => r.count)).toEqual([2, 2]);
    expect(rows.map((r) => r.player)).toEqual(["Alpha Player", "Zulu Player"]);
  });

  test("unlinked characters get their own row each and are never merged into one fake player", async () => {
    const { container } = await renderWithMembers([
      linked(10, "Real Player", 10, "Real Player", "Paladin"),
      unlinked(50, "Ghost One", "Vindicator"),
      unlinked(51, "Ghost Two", "Nestor"),
    ]);

    const rows = playerRows();
    // 3 rows, not 2 - the two unattributed characters must not collapse together
    expect(rows).toHaveLength(3);
    expect(rows.every((r) => r.count === 1)).toBe(true);

    const players = rows.map((r) => r.player);
    expect(players).toContain("Ghost One (unlinked)");
    expect(players).toContain("Ghost Two (unlinked)");
    expect(players).toContain("Real Player");

    // only the attributable character counts as a known pilot
    expect(container.textContent).toMatch(/Pilots:\s*1\b/);
    expect(container.textContent).toMatch(/Characters:\s*3\b/);
    expect(container.textContent).toMatch(/Unlinked:\s*2\b/);
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
    expect(container.textContent).toMatch(/Multiboxing:\s*2\b/);
    expect(container.textContent).toMatch(/Pilots:\s*3\b/);
    expect(container.textContent).toMatch(/Characters:\s*5\b/);
  });

  test("an empty fleet renders the section without crashing", async () => {
    const { container } = await renderWithMembers([]);

    expect(playerRows()).toHaveLength(0);
    expect(container.textContent).toMatch(/Pilots:\s*0\b/);
    expect(container.textContent).toMatch(/Characters:\s*0\b/);
    expect(container.textContent).toMatch(/Multiboxing:\s*0\b/);
  });

  // In the backend, `name` and `account_id` fall out of the same match, so a null
  // name implies a null account_id - such a character always takes the unlinked path.
  test("a character with no resolved name falls back to Unknown on the unlinked path", async () => {
    await renderWithMembers([unlinked(99, null, "Paladin")]);

    const rows = playerRows();
    expect(rows).toHaveLength(1);
    expect(rows[0].player).toBe("Unknown (unlinked)");
    expect(rows[0].characters).toBe("Unknown (Paladin)");
  });

  // account_name is looked up separately from account_id, so it can be missing on a
  // character that IS attributable. Without a fallback the Player cell renders blank,
  // which defeats the point of the table.
  test("a linked player with no resolved account name still identifies someone", async () => {
    await renderWithMembers([
      linked(10, "Visible Character", 10, null, "Paladin"),
      linked(11, "Second Character", 10, null, "Kronos"),
    ]);

    const rows = playerRows();
    expect(rows).toHaveLength(1);
    expect(rows[0].player).toBe("Visible Character");
    expect(rows[0].count).toBe(2);
  });

  test("only multiboxers get their count emphasised", async () => {
    await renderWithMembers([
      linked(10, "Boxer", 10, "Boxer", "Paladin"),
      linked(11, "Boxer Alt", 10, "Boxer", "Kronos"),
      linked(20, "Solo", 20, "Solo", "Nestor"),
    ]);

    const rows = playerRows();
    expect(rows[0]).toMatchObject({ player: "Boxer", count: 2, emphasised: true });
    expect(rows[1]).toMatchObject({ player: "Solo", count: 1, emphasised: false });
  });
});

// Regression cover: the new section is additive, so everything that was on this
// page before must still render alongside it.
describe("existing fleet page content still renders", () => {
  test("the fleet composition summary and ship breakdown are unaffected", async () => {
    const { container } = await renderWithMembers([
      linked(1, "Pally Pilot", 1, "Pally Pilot", "Paladin"),
      linked(2, "Kron Pilot", 2, "Kron Pilot", "Kronos"),
      linked(3, "Logi Pilot", 3, "Logi Pilot", "Nestor"),
    ]);

    // the pre-existing summary boxes
    expect(container.textContent).toMatch(/Marauders:\s*2\b/);
    expect(container.textContent).toMatch(/Logistics:\s*1\b/);
    expect(container.textContent).toMatch(/Vindicators:\s*0\b/);

    const shipTable = screen.getByRole("table", { name: /fleet composition/i });
    expect(within(shipTable).getByText("Ship")).toBeInTheDocument();
    expect(within(shipTable).getByText("Paladin")).toBeInTheDocument();
  });

  test("the members table still lists every character with its actions", async () => {
    await renderWithMembers([
      linked(1, "Pally Pilot", 1, "Pally Pilot", "Paladin"),
      linked(2, "Kron Pilot", 2, "Kron Pilot", "Kronos"),
    ]);

    const membersTable = screen.getByRole("table", { name: /^members$/i });
    expect(within(membersTable).getByText("Account")).toBeInTheDocument();
    // one header row + one row per character
    expect(within(membersTable).getAllByRole("row")).toHaveLength(3);
    expect(within(membersTable).getAllByText("Skills")).toHaveLength(2);
    expect(within(membersTable).getAllByText("Information")).toHaveLength(2);
  });
});
