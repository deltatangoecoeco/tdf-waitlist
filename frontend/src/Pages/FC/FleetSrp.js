import React from "react";
import { apiCall } from "../../api";

const POLL_MS = 60000;

// Wallet data older than this is flagged on the fleet page (seconds)
export const STALE_SECONDS = 90 * 60;

// Fetches the SRP status of the given fleet characters on the given waitlist and keeps it fresh
// while the tab is visible. Returns null while we don't know (loading, request failed) and when
// SRP doesn't apply to this waitlist, so the page shows nothing. Otherwise the server's answer:
// `available: false` means the SRP corp has no recent successful check, and `byId` maps a
// character id to its entry. Stops polling once the server says SRP doesn't apply here, until the
// waitlist or the fleet's members change. A failed request keeps the last answer on screen.
//
// The effect is keyed on the sorted ids, so SSE-driven refreshes that leave the same people in
// fleet don't send another request. When the ids do change the old answer stays on screen until
// the new one lands (only a different waitlist discards it), so pills don't flicker as pilots
// come and go.
export function useFleetSrp(waitlistId, characterIds) {
  const [result, setResult] = React.useState(null);
  const idsParam = [...characterIds].sort((a, b) => a - b).join(",");

  React.useEffect(() => {
    if (!idsParam || waitlistId === null || waitlistId === undefined) return;

    let cancelled = false;
    let timer = null;
    const stop = () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", loadIfVisible);
    };
    const load = () => {
      apiCall(`/api/srp/fleet?waitlist_id=${waitlistId}&character_ids=${idsParam}`, {}).then(
        (data) => {
          if (cancelled) return;
          if (!data || data.applies === false) {
            setResult(null);
            if (data) stop();
            return;
          }
          const byId = {};
          (data.characters || []).forEach((entry) => {
            byId[entry.id] = entry;
          });
          setResult({
            waitlistId,
            available: !!data.available,
            // when the wallet data was produced; a payment made after this can't show yet. Unknown
            // stays unknown: when we read it says nothing about how old ESI's data was.
            dataAsOf: data.data_as_of ?? null,
            byId,
          });
        },
        () => {}
      );
    };
    function loadIfVisible() {
      if (document.visibilityState === "visible") load();
    }

    load();
    timer = setInterval(loadIfVisible, POLL_MS);
    document.addEventListener("visibilitychange", loadIfVisible);
    return () => {
      cancelled = true;
      stop();
    };
  }, [idsParam, waitlistId]);

  // Never show another waitlist's answer, or one for a fleet that has since emptied
  return result && result.waitlistId === waitlistId && idsParam ? result : null;
}

// "paid", "checking" or "none" for one character (as built by groupCharactersByPlayer), or null
// when the server didn't answer for it. A character who joined after the wallet data was produced
// (or whose join time is unknown) can't have a visible payment yet, so it is only "checking".
export function srpStateOf(srp, character) {
  const entry = srp.byId[character.id];
  if (!entry) return null;
  if (entry.paid) return "paid";
  if (character.joinedAt === null || srp.dataAsOf === null || character.joinedAt > srp.dataAsOf) {
    return "checking";
  }
  return "none";
}

// Counts for the summary line and the names behind the "No SRP" chip
export function summariseSrp(srp, characters) {
  const summary = { paid: 0, checking: 0, none: [] };
  characters.forEach((character) => {
    const state = srpStateOf(srp, character);
    if (state === "paid") summary.paid++;
    else if (state === "checking") summary.checking++;
    else if (state === "none") summary.none.push(character);
  });
  return summary;
}
