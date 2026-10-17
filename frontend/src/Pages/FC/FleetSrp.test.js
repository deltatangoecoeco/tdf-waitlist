import { srpStateOf, summariseSrp } from "./FleetSrp";

// The rule that decides whether an FC sees an amber "No SRP" (chase them) or a muted "Checking"
// (a payment may not show yet). A wrong "No SRP" is the costly mistake.
const DATA_AS_OF = 1000;

function answer(entries, dataAsOf = DATA_AS_OF) {
  const byId = {};
  entries.forEach((entry) => {
    byId[entry.id] = entry;
  });
  return { available: true, dataAsOf, byId };
}

const character = (id, joinedAt) => ({ id, name: `Pilot ${id}`, joinedAt });

describe("srpStateOf", () => {
  const srp = answer([
    { id: 1, paid: true },
    { id: 2, paid: false },
  ]);

  test("a paid character is paid, whenever they joined", () => {
    expect(srpStateOf(srp, character(1, DATA_AS_OF + 500))).toBe("paid");
  });

  test("unpaid and in fleet before the wallet data: No SRP", () => {
    expect(srpStateOf(srp, character(2, DATA_AS_OF - 1))).toBe("none");
    expect(srpStateOf(srp, character(2, DATA_AS_OF))).toBe("none");
  });

  test("unpaid but joined after the wallet data: Checking", () => {
    expect(srpStateOf(srp, character(2, DATA_AS_OF + 1))).toBe("checking");
  });

  test("an unknown join time or data age is never No SRP", () => {
    expect(srpStateOf(srp, character(2, null))).toBe("checking");
    expect(srpStateOf(answer([{ id: 2, paid: false }], null), character(2, 0))).toBe("checking");
  });

  test("a character the server didn't answer for gets no pill", () => {
    expect(srpStateOf(srp, character(3, 0))).toBeNull();
  });
});

test("summariseSrp counts each state and names the No SRP pilots", () => {
  const srp = answer([
    { id: 1, paid: true },
    { id: 2, paid: false },
    { id: 3, paid: false },
  ]);
  const summary = summariseSrp(srp, [
    character(1, 0),
    character(2, 0),
    character(3, DATA_AS_OF + 60),
    character(4, 0),
  ]);
  expect(summary.paid).toBe(1);
  expect(summary.checking).toBe(1);
  expect(summary.none.map((c) => c.id)).toEqual([2]);
});
