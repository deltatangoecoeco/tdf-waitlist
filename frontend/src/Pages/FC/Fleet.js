import React from "react";
import { AuthContext, ToastContext, WaitlistContext } from "../../contexts";
import { Confirm } from "../../Components/Modal";
import { Button, Buttons, InputGroup, NavButton, Select } from "../../Components/Form";
import { Content, Title } from "../../Components/Page";
import { apiCall, errorToaster, toaster, useApi } from "../../api";
import { Cell, CellHead, Row, Table, TableBody, TableHead } from "../../Components/Table";
import { BorderedBox } from "../../Components/NoteBox";
import { sortBy, entries } from "lodash";
import { useNavigate, useLocation } from "react-router";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faRotate } from "@fortawesome/free-solid-svg-icons";
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
    const character = { name, ship: member.ship.name };

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

// Memoised on `members`, which only changes identity when the fleet is refetched.
// The parent re-renders on unrelated state (modal toggles, the refresh button), so
// this skips both the render and the reconciliation of every row in between.
const CharactersPerPlayer = React.memo(function CharactersPerPlayer({ members }) {
  const { rows, characterCount, pilotCount, multiboxingCount, unlinkedCount } =
    groupCharactersByPlayer(members);

  return (
    <>
      <br />
      <Title>Characters per player</Title>
      <InputGroup>
        <BorderedBox>Pilots: {pilotCount} </BorderedBox>
        <BorderedBox>Characters: {characterCount} </BorderedBox>
        <BorderedBox>Multiboxing: {multiboxingCount} </BorderedBox>
        {unlinkedCount > 0 && <BorderedBox>Unlinked: {unlinkedCount} </BorderedBox>}
      </InputGroup>
      <Table fullWidth aria-label="Characters per player">
        <TableHead>
          <Row>
            <CellHead>Player</CellHead>
            <CellHead>In fleet</CellHead>
            <CellHead>Characters</CellHead>
          </Row>
        </TableHead>
        <TableBody>
          {rows.map((row) => (
            <Row key={row.key}>
              <Cell>{row.unlinked ? `${row.player} (unlinked)` : row.player}</Cell>
              <Cell>
                {row.count >= MULTIBOX_THRESHOLD ? <strong>{row.count}</strong> : row.count}
              </Cell>
              <Cell>
                {row.characters
                  .map((character) => `${character.name} (${character.ship})`)
                  .join(", ")}
              </Cell>
            </Row>
          ))}
        </TableBody>
      </Table>
    </>
  );
});

export function FleetMembers({refreshedAt}) {
  const authContext = React.useContext(AuthContext);
  const [fleetMembers, setFleetMembers] = React.useState(null);
  const characterId = authContext.current.id;
  React.useEffect(() => {
    setFleetMembers(null);
    apiCall("/api/fleet/members?character_id=" + characterId, {})
      .then(setFleetMembers)
      .catch((err) => setFleetMembers(null)); // What's error handling?
  }, [characterId, refreshedAt]);

  if (!fleetMembers) {
    return null;
  }
  var cats = {
    Marauder: 0,
    Logi: 0,
    Vindicator: 0,
    "Mega/Night": 0,
  };

  var summary = {};
  if (fleetMembers) {
    fleetMembers.members.forEach((member) => {
      if (!summary[member.ship.name]) summary[member.ship.name] = 0;
      summary[member.ship.name]++;
      if (marauders.includes(member.ship.name)) cats["Marauder"]++;
      if (logi.includes(member.ship.name)) cats["Logi"]++;
      if ("Vindicator" === member.ship.name) cats["Vindicator"]++;
      if (bad.includes(member.ship.name)) cats["Mega/Night"]++;
    });
  }
  return (
    <>
      <br />
      <Title>Fleet composition</Title>
      <InputGroup>
        <BorderedBox>Marauders: {cats["Marauder"]} </BorderedBox>
        <BorderedBox>Logistics: {cats["Logi"]} </BorderedBox>
        <BorderedBox>Vindicators: {cats["Vindicator"]} </BorderedBox>
        <BorderedBox>Megathron/Nightmare: {cats["Mega/Night"]} </BorderedBox>
      </InputGroup>
      <Table aria-label="Fleet composition">
        <TableHead>
          <Row>
            <CellHead>Ship</CellHead>
            <CellHead>#</CellHead>
          </Row>
        </TableHead>
        <TableBody>
          {sortBy(entries(summary), [1]).map(([shipName, count]) => (
            <Row key={shipName}>
              <Cell>{shipName}</Cell>
              <Cell>{count}</Cell>
            </Row>
          ))}
        </TableBody>
      </Table>
      <br />
      <Title>Members</Title>
      <Table fullWidth aria-label="Members">
        <TableHead>
          <Row>
            <CellHead>Name</CellHead>
            <CellHead>Ship</CellHead>
            <CellHead>Account</CellHead>
            <CellHead>Actions</CellHead>
          </Row>
        </TableHead>
        <TableBody>
          {fleetMembers &&
            fleetMembers.members.map((member) => (
              <Row key={member.id}>
                <Cell>{member.name}</Cell>
                <Cell>{member.ship.name}</Cell>
                <Cell>{member.account_name}</Cell>
                <Cell>
                  <NavButton to={"/skills?character_id=" + member.id}>Skills</NavButton>
                  <NavButton to={"/pilot?character_id=" + member.id}>Information</NavButton>
                </Cell>
              </Row>
            ))}
        </TableBody>
      </Table>

      <CharactersPerPlayer members={fleetMembers.members} />
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
