import React from "react";
import { AuthContext, ToastContext, WaitlistContext } from "../../contexts";
import { Confirm } from "../../Components/Modal";
import { Button, Buttons, InputGroup, NavButton, Select } from "../../Components/Form";
import { Content } from "../../Components/Page";
import { apiCall, errorToaster, toaster, useApi } from "../../api";
import { sortBy, entries } from "lodash";
import { useNavigate, useLocation } from "react-router";
import { Link } from "react-router-dom";
import styled from "styled-components";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faGraduationCap, faRotate } from "@fortawesome/free-solid-svg-icons";
import { useTranslation } from "react-i18next";

const marauders = ["Paladin", "Kronos"];
const logi = ["Nestor", "Guardian", "Oneiros"];
const bad = ["Megathron", "Nightmare"];

async function setWaitlistOpen(waitlistId, isOpen) {
  return await apiCall("/api/waitlist/set_open", {
    json: { waitlist_id: waitlistId, open: isOpen },
  });
}

async function emptyWaitlist(waitlistId) {
  return await apiCall("/api/waitlist/empty", {
    json: { waitlist_id: waitlistId },
  });
}

async function closeFleet(characterId) {
  return await apiCall("/api/fleet/close", {
    json: { character_id: characterId },
  });
}

export function Fleet() {
  const [fleetCloseModalOpen, setFleetCloseModalOpen] = React.useState(false);
  const [emptyWaitlistModalOpen, setEmptyWaitlistModalOpen] = React.useState(false);
  const [refreshedAt, setRefreshedAt] = React.useState(Date.now());
  const authContext = React.useContext(AuthContext);
  const toastContext = React.useContext(ToastContext);
  const waitlistContext = React.useContext(WaitlistContext);
  const waitlistId = waitlistContext !== null ? waitlistContext.active : null;
  const { t } = useTranslation("fleet");
  const [fleets, refreshFleet] = useApi("/api/fleet/status");
  const location = useLocation();

  React.useEffect(() => {
    if (waitlistId !== null) {
      const params = new URLSearchParams(location.search);
      params.set("wl", waitlistId);
      window.history.replaceState({}, "", `${location.pathname}?${params.toString()}`);
    }
  }, [waitlistId, location]);

  React.useEffect(() => {
    // FCs will need this, request it now
    if (window.Notification && Notification.permission === "default") {
      Notification.requestPermission();
    }
  }, []);

  if (waitlistContext === null || waitlistContext.available === null || !waitlistContext.available.find(n => n.id === waitlistContext.active)) {
    return (<>{t("loading")}</>);
  }

  const currentWaitlist = waitlistContext.available.find(n => n.id === waitlistContext.active);

  return (
    <>
      <Buttons>
        <NavButton to="/fc/fleet/register">{t("load_fleet")}</NavButton>
        {/* <NavButton to="/auth/start/fc">ESI re-auth as FC</NavButton> */}
        <InputGroup>
          <Button variant={currentWaitlist.open ? 'success' : ''} onClick={() => toaster(toastContext, setWaitlistOpen(waitlistContext.active, true))}>
            {t("open")}
          </Button>
          <Button variant={!currentWaitlist.open ? 'danger' : ''} onClick={() => toaster(toastContext, setWaitlistOpen(waitlistContext.active, false))}>
            {t("close")}
          </Button>
        </InputGroup>
        <Button onClick={() => setEmptyWaitlistModalOpen(true)}>{t("clear_waitlist")}</Button>
        {fleets && fleets.fleets.length > 0 && fleets.fleets.some(n => authContext.current.id === n.boss.id) && 
        <Button variant="danger" onClick={() => setFleetCloseModalOpen(true)}>
          {t("kick_everyone")}
          </Button>}
        <Button onClick={() => { refreshFleet(); setRefreshedAt(Date.now()); }}><FontAwesomeIcon icon={faRotate} /></Button>
      </Buttons>
      <Content>
        <p>
          Make sure you re-auth via ESI, then create an in-game fleet with your comp. Click the
          &quot;Configure fleet&quot; button, and select the five squads that the tool will invite
          people into. Then open the waitlist, allowing people to X up.
        </p>
        <p>
          To hand over the fleet, transfer the star (Boss role). Then the new FC should go via
          &quot;Configure fleet&quot; again, as if it was a new fleet.
        </p>
        {!fleets
          ? null
          : fleets.fleets.map((fleet) => (
              <div key={fleet.id}>
              Fleet <code>{fleet.id}</code> (<code>{fleet.boss.name}</code>)
              </div>
            ))}
      </Content>
      {authContext.access["fleet-comp-history"] && (
        <Buttons>
          <NavButton to="/fc/fleet-comp-history">Fleet comp history</NavButton>
        </Buttons>
      )}

      {fleets && fleets.fleets.length > 0 && fleets.fleets.some(n => authContext.current.id === n.boss.id) && <FleetMembers refreshedAt={refreshedAt} />}
      <Confirm
        open={fleetCloseModalOpen}
        setOpen={setFleetCloseModalOpen}
        title="Kick everyone from fleet"
        onConfirm={(evt) =>
          toaster(toastContext, closeFleet(authContext.current.id)).finally(() =>
            setFleetCloseModalOpen(false)
          )
        }
      >
        Are you sure?
      </Confirm>
      <Confirm
        open={emptyWaitlistModalOpen}
        setOpen={setEmptyWaitlistModalOpen}
        title="Empty waitlist"
        onConfirm={(evt) =>
          toaster(toastContext, emptyWaitlist(waitlistContext.active)).finally(() => setEmptyWaitlistModalOpen(false))
        }
      >
        Are you sure?
      </Confirm>
    </>
  );
}

async function registerFleet({ fleetInfo, categoryMatches, authContext }) {
  return await apiCall("/api/fleet/register", {
    json: {
      character_id: authContext.current.id,
      assignments: categoryMatches,
      fleet_id: fleetInfo.fleet_id,
    },
  });
}

// A player flying this many characters or more is worth asking to drop one.
const MULTIBOX_THRESHOLD = 2;
// From this many characters a player gets a warning-coloured count pill.
const HEAVY_MULTIBOX_THRESHOLD = 3;

// Groups the characters currently in fleet by the player who owns them, so an FC
// can see who is flying several at once. Characters that have never authenticated
// with the site have no account_id and cannot be attributed, so each becomes its
// own row rather than being merged into a single phantom player.
// Returns structured data only - formatting is left to the component.
function groupCharactersByPlayer(members) {
  const byPlayer = {};
  const unlinkedRows = [];

  members.forEach((member) => {
    const name = member.name || "Unknown";
    const character = { id: member.id, name, ship: member.ship.name };

    if (member.account_id === null || member.account_id === undefined) {
      unlinkedRows.push({
        key: "unlinked-" + member.id,
        player: name,
        unlinked: true,
        count: 1,
        characters: [character],
      });
      return;
    }

    if (!byPlayer[member.account_id]) {
      byPlayer[member.account_id] = {
        // account_name is resolved separately from account_id and can be missing;
        // fall back to a character name so the row still identifies someone
        player: member.account_name || name,
        count: 0,
        characters: [],
      };
    }
    byPlayer[member.account_id].count++;
    byPlayer[member.account_id].characters.push(character);
  });

  const playerRows = entries(byPlayer).map(([accountId, data]) => ({
    key: "account-" + accountId,
    player: data.player,
    unlinked: false,
    count: data.count,
    characters: data.characters,
  }));

  return {
    // most characters first, then by name so equal counts stay in a stable order
    rows: sortBy(playerRows.concat(unlinkedRows), [(row) => -row.count, "player"]),
    characterCount: members.length,
    pilotCount: playerRows.length,
    multiboxingCount: playerRows.filter((row) => row.count >= MULTIBOX_THRESHOLD).length,
    unlinkedCount: unlinkedRows.length,
  };
}

const Chips = styled.div`
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 0.5em;
`;

const Summary = styled.div`
  color: ${(props) => props.theme.colors.accent4};
  font-size: 0.85em;
  margin-bottom: 0.6em;
`;

const SectionLabel = styled.div`
  color: ${(props) => props.theme.colors.accent4};
  font-size: 0.85em;
  font-weight: 600;
  margin: 0.8em 0 0.4em;
`;

const CardGrid = styled.div`
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
  gap: 14px;
  align-items: start;
`;

// Modelled on the x-up card (XCardDOM in Pages/Waitlist/XCard.js)
const PlayerCard = styled.div`
  border: solid 2px ${(props) => props.theme.colors.secondary.color};
  background-color: ${(props) => props.theme.colors.secondary.color};
  color: ${(props) => props.theme.colors.secondary.text};
  border-radius: 5px;
  font-size: 0.9em;
  filter: drop-shadow(0px 4px 5px ${(props) => props.theme.colors.shadow});
`;

const CardHead = styled.div`
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0.3em 0.3em 0.35em 0.6em;
`;

const PlayerName = styled.span`
  flex: 1;
  min-width: 0;
  font-weight: 600;
  word-break: break-word;
`;

const Pill = styled.span`
  font-size: 11px;
  padding: 1px 7px;
  border-radius: 4px;
  white-space: nowrap;
  background-color: ${(props) => props.theme.colors.background};
  color: ${(props) => (props.muted ? props.theme.colors.accent4 : props.theme.colors.text)};
  border: 1px solid ${(props) => props.theme.colors.accent2};
  ${(props) =>
    props.heavy &&
    `
    background-color: ${props.theme.colors.warning.color};
    color: ${props.theme.colors.warning.text};
    border-color: ${props.theme.colors.warning.color};
  `}
`;

const CardBody = styled.div`
  background-color: ${(props) => props.theme.colors.background};
  color: ${(props) => props.theme.colors.text};
  border-radius: 0 0 3px 3px;
`;

const CharacterLine = styled.div`
  display: grid;
  grid-template-columns: 82px 1fr auto;
  gap: 8px;
  padding: 5px 10px;
  align-items: center;
  & + & {
    border-top: 1px solid ${(props) => props.theme.colors.accent1};
  }
`;

const MutedText = styled.span`
  color: ${(props) => props.theme.colors.accent4};
`;

// The character's name links to their pilot page; it only underlines on hover so the
// lists stay readable
const PilotLink = styled(Link)`
  color: inherit;
  text-decoration: none;
  &:hover {
    text-decoration: underline;
  }
`;

const IconLink = styled(Link)`
  color: ${(props) => props.theme.colors.accent4};
  text-decoration: none;
  &:hover {
    color: ${(props) => props.theme.colors.text};
  }
`;

const ElseList = styled.div`
  margin-top: 10px;
  font-size: 0.9em;
  background-color: ${(props) => props.theme.colors.background};
  color: ${(props) => props.theme.colors.text};
  border: 1px solid ${(props) => props.theme.colors.accent2};
  border-radius: 5px;
`;

// Fixed name and ship columns so ships line up down the list, as they do in the cards
const ElseLine = styled.div`
  display: grid;
  grid-template-columns: minmax(0, 14em) minmax(0, 11em) 1fr auto;
  align-items: center;
  gap: 8px;
  padding: 5px 10px;
  & + & {
    border-top: 1px solid ${(props) => props.theme.colors.accent1};
  }
`;

function SkillsLink({ id }) {
  return (
    <IconLink to={"/skills?character_id=" + id} title="Skills" aria-label="Skills">
      <FontAwesomeIcon icon={faGraduationCap} />
    </IconLink>
  );
}

function CompositionStrip({ cats, grouping }) {
  const { characterCount, pilotCount, unlinkedCount } = grouping;
  return (
    <>
      <Chips>
        <Pill>
          <b>{cats["Marauder"]}</b> Marauders
        </Pill>
        <Pill>
          <b>{cats["Logi"]}</b> Logistics
        </Pill>
        <Pill>
          <b>{cats["Vindicator"]}</b> Vindicators
        </Pill>
        <Pill>
          <b>{cats["Mega/Night"]}</b> Megathron/Nightmare
        </Pill>
      </Chips>
      <Summary>
        {characterCount} characters · {pilotCount} pilots
        {unlinkedCount > 0 && ` · ${unlinkedCount} unlinked`}
      </Summary>
    </>
  );
}

// Memoised on `grouping`, which only changes identity when the fleet is refetched.
// The parent re-renders on unrelated state (modal toggles, the refresh button), so
// this skips both the render and the reconciliation of every row in between.
const MultiboxerCards = React.memo(function MultiboxerCards({ grouping }) {
  const rows = grouping.rows.filter((row) => row.count >= MULTIBOX_THRESHOLD);

  return (
    <>
      <SectionLabel>Multiboxers ({grouping.multiboxingCount})</SectionLabel>
      {rows.length > 0 ? (
        <CardGrid role="list" aria-label="Multiboxers">
          {rows.map((row) => (
            <PlayerCard role="listitem" key={row.key}>
              <CardHead>
                <PlayerName>{row.player}</PlayerName>
                <Pill heavy={row.count >= HEAVY_MULTIBOX_THRESHOLD}>
                  <b>{row.count}</b> characters
                </Pill>
              </CardHead>
              <CardBody>
                {row.characters.map((character) => (
                  <CharacterLine key={character.id}>
                    <strong>{character.ship}</strong>
                    <PilotLink to={"/pilot?character_id=" + character.id}>
                      {character.name === row.player ? (
                        <MutedText>
                          <em>main</em>
                        </MutedText>
                      ) : (
                        character.name
                      )}
                    </PilotLink>
                    <SkillsLink id={character.id} />
                  </CharacterLine>
                ))}
              </CardBody>
            </PlayerCard>
          ))}
        </CardGrid>
      ) : (
        <p>Nobody is flying more than one character.</p>
      )}
    </>
  );
});

// Single-character players and unlinked characters: collapsed by default, and a compact list
// rather than cards because there is nothing to compare between them.
const EveryoneElse = React.memo(function EveryoneElse({ grouping }) {
  const [showAll, setShowAll] = React.useState(false);
  const rows = grouping.rows.filter((row) => row.count < MULTIBOX_THRESHOLD);

  if (rows.length === 0) {
    return null;
  }

  return (
    <div style={{ marginTop: "14px" }}>
      <Button onClick={() => setShowAll(!showAll)}>
        {showAll ? "Hide pilots on one character" : `+${rows.length} pilots on one character`}
      </Button>
      {showAll && (
        <ElseList role="list" aria-label="Everyone else">
          {rows.map((row) =>
            row.characters.map((character) => (
              <ElseLine role="listitem" key={character.id}>
                <PilotLink to={"/pilot?character_id=" + character.id}>{character.name}</PilotLink>
                <strong>{character.ship}</strong>
                <span>{row.unlinked && <Pill muted>unlinked</Pill>}</span>
                <SkillsLink id={character.id} />
              </ElseLine>
            ))
          )}
        </ElseList>
      )}
    </div>
  );
});

function FleetMembers({refreshedAt}) {
  const authContext = React.useContext(AuthContext);
  const [fleetMembers, setFleetMembers] = React.useState(null);
  const characterId = authContext.current.id;
  React.useEffect(() => {
    setFleetMembers(null);
    apiCall("/api/fleet/members?character_id=" + characterId, {})
      .then(setFleetMembers)
      .catch((err) => setFleetMembers(null)); // What's error handling?
  }, [characterId, refreshedAt]);

  const members = fleetMembers ? fleetMembers.members : null;
  const grouping = React.useMemo(
    () => (members ? groupCharactersByPlayer(members) : null),
    [members]
  );

  if (!fleetMembers) {
    return null;
  }
  var cats = {
    Marauder: 0,
    Logi: 0,
    Vindicator: 0,
    "Mega/Night": 0,
  };

  members.forEach((member) => {
    if (marauders.includes(member.ship.name)) cats["Marauder"]++;
    if (logi.includes(member.ship.name)) cats["Logi"]++;
    if ("Vindicator" === member.ship.name) cats["Vindicator"]++;
    if (bad.includes(member.ship.name)) cats["Mega/Night"]++;
  });
  return (
    <>
      <br />
      <CompositionStrip cats={cats} grouping={grouping} />
      <MultiboxerCards grouping={grouping} />
      <EveryoneElse grouping={grouping} />
    </>
  );
}

function detectSquads({ matches, categories, wings }) {
  var newMatches = { ...matches };
  var hadChanges = false;
  for (const category of categories) {
    if (!(category.id in matches)) {
      for (const wing of wings) {
        if (wing.name.match(/on\s+grid/i)) {
          for (const squad of wing.squads) {
            if (
              squad.name.toLowerCase().includes(category.name.toLowerCase()) ||
              squad.name.toLowerCase().includes(category.id.toLowerCase())
            ) {
              newMatches[category.id] = [wing.id, squad.id];
              hadChanges = true;
            }
          }
        }
      }
    }
  }
  if (hadChanges) {
    return newMatches;
  }
  return null;
}

export function FleetRegister() {
  const authContext = React.useContext(AuthContext);
  const toastContext = React.useContext(ToastContext);
  const [fleetInfo, setFleetInfo] = React.useState(null);
  const [categories, setCategories] = React.useState(null);
  const [categoryMatches, setCategoryMatches] = React.useState({});
  const navigate = useNavigate();

  const characterId = authContext.current.id;
  React.useEffect(() => {
    setFleetInfo(null);
    errorToaster(
      toastContext,
      apiCall("/api/fleet/info?character_id=" + characterId, {}).then(setFleetInfo)
    );

    setCategories(null);
    errorToaster(
      toastContext,
      apiCall("/api/categories", {}).then((data) => setCategories(data.categories))
    );
  }, [characterId, toastContext]);

  React.useEffect(() => {
    if (!categories || !fleetInfo) return;

    var newMatches = detectSquads({
      matches: categoryMatches,
      categories,
      wings: fleetInfo.wings,
    });
    if (newMatches) {
      setCategoryMatches(newMatches);
    }
  }, [fleetInfo, categories, categoryMatches, setCategoryMatches]);

  if (!fleetInfo || !categories) {
    return <em>Loading fleet information...</em>;
  }

  return (
    <>
      <CategoryMatcher
        categories={categories}
        wings={fleetInfo.wings}
        value={categoryMatches}
        onChange={setCategoryMatches}
      />
      <Button
        variant="primary"
        onClick={(evt) => 
          toaster(toastContext, registerFleet({ authContext, fleetInfo, categoryMatches }))
          .then(() => navigate("/fc/fleet")) 
        }
      >
        Continue
      </Button>
    </>
  );
}

function CategoryMatcher({ categories, wings, onChange, value }) {
  var flatSquads = [];
  wings.forEach((wing) => {
    wing.squads.forEach((squad) => {
      flatSquads.push({
        name: `${wing.name} - ${squad.name}`,
        id: `${wing.id},${squad.id}`,
      });
    });
  });

  var catDom = [];
  for (const category of categories) {
    var squadSelection = flatSquads.map((squad) => (
      <option key={squad.id} value={squad.id}>
        {squad.name}
      </option>
    ));
    catDom.push(
      <p key={category.id}>
        <label className="label">
          {category.name}
          <br />
        </label>
        <Select
          value={value[category.id]}
          onChange={(evt) =>
            onChange({
              ...value,
              [category.id]: evt.target.value.split(",").map((i) => parseInt(i)),
            })
          }
        >
          <option></option>
          {squadSelection}
        </Select>
      </p>
    );
  }
  return <Content>{catDom}</Content>;
}
