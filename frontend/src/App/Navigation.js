import React from "react";
import { useLocation } from "react-router-dom";
import { NavLink } from "react-router-dom";
import styled from "styled-components";
import { MobileButton } from "../Components/Form";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faBars, faTimes } from "@fortawesome/free-solid-svg-icons";
import { useTranslation } from 'react-i18next';

const Links = styled(NavLink).attrs((props) => ({
  className: props.isActive ? "active" : "",
}))`
  padding: 1em;
  color: ${(props) => props.theme.colors.accent4};
  text-decoration: none;
  &:hover {
    color: ${(props) => props.theme.colors.text};
    background-color: ${(props) => props.theme.colors.accent1};
    border-radius: 2px;
  }
  &.active {
    color: ${(props) => props.theme.colors.active};
  }
  @media (max-width: 480px) {
    &.active {
      background-color: ${(props) => props.theme.colors.accent2};
      border-radius: 4px;
    }
  }
`;

const MobileButtonDOM = styled.div`
  * {
    display: ${(props) => (props.open ? "flex" : "none")};
    flex-wrap: wrap;
    flex-direction: column;
  }
`;

const Content = styled.div`
  padding: 0.3em;
  width: 100%;
  transition-delay: 2s;
  opacity: ${(props) => (props.open ? "1" : "0")};
  height: ${(props) => (props.open ? "100%" : "0")};
  transition: all 0.3s;
  background-color: ${(props) => props.theme.colors.accent1};
  border-radius: 5px;
`;

export function MobileNav({ isOpen, whoami }) {
  return (
    <>
      <Content open={isOpen}>
        <MobileButtonDOM open={isOpen}>
          <NavLinks whoami={whoami} />
        </MobileButtonDOM>
      </Content>
    </>
  );
}

export function MobileNavButton({ isOpen, setIsOpen }) {
  const location = useLocation();
  React.useEffect(() => {
    setIsOpen(false);
  }, [location, setIsOpen]);
  return (
    <>
      {isOpen ? (
        <MobileButton onClick={(evt) => setIsOpen(false)}>
          <FontAwesomeIcon icon={faTimes} />
        </MobileButton>
      ) : (
        <MobileButton onClick={(evt) => setIsOpen(true)}>
          <FontAwesomeIcon icon={faBars} />
        </MobileButton>
      )}
    </>
  );
}

export function NavLinks({ whoami }) {
  const { t } = useTranslation('navigation');

  return (
    <>
      {whoami && (
        <>
          <Links end to="/waitlist">
            {t("waitlist")}
          </Links>
          <Links end to="/skills">
            {t("skills")}
          </Links>
          <Links end to="/pilot">
            {t("pilot")}
          </Links>
        </>
      )}
      {/* <Links end to="/guide">
        Guides
      </Links>
      <Links end to="/fits">
        Fits
      </Links> */}
      {/* <Links end to="/isk-h/calc">
        {t('isk_h')}
      </Links> */}
      {whoami && whoami.access["fleet-view"] && (
        <Links end to="/fc/fleet">
          {t("fleet")}
        </Links>
      )}
      {/* SRP managers who aren't FCs still need the way to the SRP wallet page */}
      {whoami && (whoami.access["fleet-view"] || whoami.access["srp-manage"]) && (
        <Links end to="/fc">
          {t("fc")}
        </Links>
      )}
      {whoami && whoami.access["search"] && (
        <Links end to="/fc/search">
          {t("search")}
        </Links>
      )}
    </>
  );
}
