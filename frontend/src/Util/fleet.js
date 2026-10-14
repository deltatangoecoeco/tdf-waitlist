// Whether characterId is the registered boss of a fleet the waitlist is tracking - of a
// particular fleet, if fleetId is given.
export function isRegisteredBoss(fleets, characterId, fleetId) {
  return fleets.some(
    (fleet) => fleet.boss.id === characterId && (fleetId === undefined || fleet.id === fleetId)
  );
}
