// Translations of the demo page: English (the source), Portuguese, German.
// UI maps static English text nodes to their translation; T holds every text
// the script writes; the long sections live in blocks.js.

export const LANGS = { en: "English", pt: "Português", de: "Deutsch" };
export const LOCALE = { en: "en-GB", pt: "pt-BR", de: "de-DE" };

export const UI = {
  pt: {
    "Source code and README": "Código-fonte e README", "Feeder calculator": "Calculadora do alimentador", "How it works": "Como funciona",
    "The MPC study": "O estudo do MPC", "Country rules": "Regras dos países", "IEC 104 point list": "Lista de pontos IEC 104",
    "This demo needs JavaScript and WebAssembly.": "Esta demonstração precisa de JavaScript e WebAssembly.",
    "DSO control centre": "Centro de controle da distribuidora",
    "You are the grid operator. Every button sends a real IEC 104 command to the site; the frames appear in the log below.": "Você é a distribuidora. Cada botão envia um comando IEC 104 real para o local; os quadros aparecem no registro abaixo.",
    "Rules the site runs under": "Regras aplicadas ao local", "Germany · §14a EnWG": "Alemanha · §14a EnWG", "Austria · ElWG": "Áustria · ElWG", "Switzerland · StromVG": "Suíça · StromVG",
    "Day": "Dia", "real German day-ahead prices, bidding zone DE-LU (SMARD)": "preços reais do mercado alemão do dia seguinte, zona DE-LU (SMARD)",
    "Winter · 20 Jan 2025": "Inverno · 20 jan 2025", "Spring · 6 Apr 2025": "Primavera · 6 abr 2025",
    "Controller": "Controle", "switch any time; a rules-only copy runs alongside": "troque quando quiser; uma cópia só com regras roda ao lado",
    "Rules only": "Só regras", "MPC planner": "Planejador MPC", "MPC, chance-constrained": "MPC probabilístico",
    "Reduce consumption": "Reduzir consumo", "Normal": "Normal", "Dim": "Reduzir", "Feed-in limit": "Limite de injeção",
    "C_SE_NC_1 · IOA 5002 · % of 120 kWp": "C_SE_NC_1 · IOA 5002 · % de 120 kWp", "Emergency": "Emergência",
    "C_SC_NA_1 · IOA 5003 · overrides day limits, budgets, opt-outs": "C_SC_NA_1 · IOA 5003 · passa por cima de limites, orçamentos e recusas",
    "Off": "Desligada", "Immediate threat": "Ameaça imediata", "Simulated time": "Tempo simulado",
    "a logistics depot, 06:00 to 06:00 next day": "um depósito logístico, das 06:00 às 06:00 do dia seguinte",
    "Jump to a scene": "Ir para uma cena", "06:00 · the whole day and night": "06:00 · o dia e a noite inteiros",
    "11:30 · solar peak, try a feed-in limit": "11:30 · pico solar, teste um limite de injeção", "16:15 · vans arrive, price peak, try dimming": "16:15 · as vans chegam, pico de preço, teste reduzir",
    "Power at the depot, kW": "Potência no depósito, kW", "Grid connection (+ import, − export)": "Ligação com a rede (+ compra, − venda)",
    "PV output": "Produção solar", "Chargers + heat pump": "Carregadores + bomba de calor", "Battery (+ charging)": "Bateria (+ carregando)", "DSO limit in force": "Limite da distribuidora",
    "Day-ahead price, €/MWh": "Preço do dia seguinte, €/MWh", "winter peak 583 at 17:00 · spring low −115 at 14:00": "pico no inverno 583 às 17:00 · mínimo na primavera −115 às 14:00",
    "Battery state of charge, %": "Carga da bateria, %", "actual": "real", "plan": "plano", "Indoor temperature, °C": "Temperatura interna, °C", "comfort floor": "mínimo de conforto",
    "This site vs the same site on rules alone": "Este local vs o mesmo local só com regras", "Consumption limit: controllable devices' grid draw": "Limite de consumo: potência dos controláveis tirada da rede",
    "Free curtailment budget (CH, 3% of yearly PV)": "Orçamento de corte gratuito (CH, 3% da energia solar do ano)", "Fault injection": "Injeção de falhas",
    "Meter offline": "Medidor fora do ar", "Charger 2 offline": "Carregador 2 fora do ar", "Heat pump offline": "Bomba de calor fora do ar", "Inverter 1 offline": "Inversor 1 fora do ar", "Battery offline": "Bateria fora do ar",
    "The device stops answering Modbus. Watch the fallbacks: a charger drops to its own 6 A failsafe and the battery goes idle after 30 s; without the meter the budget shrinks to the floor.": "O equipamento para de responder ao Modbus. Veja os modos de segurança: um carregador cai sozinho para 6 A e a bateria para depois de 30 s; sem o medidor, o limite encolhe até o piso.",
    "Field devices (Modbus TCP) — setpoint written by the gateway → measured": "Equipamentos (Modbus TCP) — valor escrito pelo gateway → medido",
    "IEC 60870-5-104 frames on the DSO link": "Quadros IEC 60870-5-104 no enlace com a distribuidora", "show raw bytes": "mostrar bytes",
  },
  de: {
    "Source code and README": "Quellcode und README", "Feeder calculator": "Strang-Rechner", "How it works": "So funktioniert es",
    "The MPC study": "Die MPC-Studie", "Country rules": "Länderregeln", "IEC 104 point list": "IEC-104-Datenpunktliste",
    "This demo needs JavaScript and WebAssembly.": "Diese Demo benötigt JavaScript und WebAssembly.",
    "DSO control centre": "Netzleitstelle",
    "You are the grid operator. Every button sends a real IEC 104 command to the site; the frames appear in the log below.": "Sie sind der Netzbetreiber. Jede Schaltfläche sendet einen echten IEC-104-Befehl an den Standort; die Telegramme erscheinen im Protokoll unten.",
    "Rules the site runs under": "Geltende Regeln", "Germany · §14a EnWG": "Deutschland · §14a EnWG", "Austria · ElWG": "Österreich · ElWG", "Switzerland · StromVG": "Schweiz · StromVG",
    "Day": "Tag", "real German day-ahead prices, bidding zone DE-LU (SMARD)": "echte deutsche Day-Ahead-Preise, Gebotszone DE-LU (SMARD)",
    "Winter · 20 Jan 2025": "Winter · 20. Jan. 2025", "Spring · 6 Apr 2025": "Frühling · 6. Apr. 2025",
    "Controller": "Regelung", "switch any time; a rules-only copy runs alongside": "jederzeit umschaltbar; eine regelbasierte Kopie läuft mit",
    "Rules only": "Nur Regeln", "MPC planner": "MPC-Planer", "MPC, chance-constrained": "MPC, chance-constrained",
    "Reduce consumption": "Bezug reduzieren", "Normal": "Normal", "Dim": "Dimmen", "Feed-in limit": "Einspeisegrenze",
    "C_SE_NC_1 · IOA 5002 · % of 120 kWp": "C_SE_NC_1 · IOA 5002 · % von 120 kWp", "Emergency": "Notfall",
    "C_SC_NA_1 · IOA 5003 · overrides day limits, budgets, opt-outs": "C_SC_NA_1 · IOA 5003 · setzt Tageslimits, Budgets und Widersprüche außer Kraft",
    "Off": "Aus", "Immediate threat": "Akute Gefahr", "Simulated time": "Simulierte Zeit",
    "a logistics depot, 06:00 to 06:00 next day": "ein Logistikdepot, 06:00 bis 06:00 am Folgetag",
    "Jump to a scene": "Zu einer Szene springen", "06:00 · the whole day and night": "06:00 · der ganze Tag und die Nacht",
    "11:30 · solar peak, try a feed-in limit": "11:30 · Solarspitze, probieren Sie eine Einspeisegrenze", "16:15 · vans arrive, price peak, try dimming": "16:15 · Transporter kommen an, Preisspitze, probieren Sie das Dimmen",
    "Power at the depot, kW": "Leistung am Depot, kW", "Grid connection (+ import, − export)": "Netzanschluss (+ Bezug, − Einspeisung)",
    "PV output": "PV-Leistung", "Chargers + heat pump": "Ladepunkte + Wärmepumpe", "Battery (+ charging)": "Batterie (+ Laden)", "DSO limit in force": "Grenze des Netzbetreibers",
    "Day-ahead price, €/MWh": "Day-Ahead-Preis, €/MWh", "winter peak 583 at 17:00 · spring low −115 at 14:00": "Winterspitze 583 um 17:00 · Frühlingstief −115 um 14:00",
    "Battery state of charge, %": "Ladezustand der Batterie, %", "actual": "Ist", "plan": "Plan", "Indoor temperature, °C": "Innentemperatur, °C", "comfort floor": "Komfortgrenze",
    "This site vs the same site on rules alone": "Dieser Standort vs. derselbe Standort nur mit Regeln", "Consumption limit: controllable devices' grid draw": "Bezugsgrenze: Netzbezug der steuerbaren Verbraucher",
    "Free curtailment budget (CH, 3% of yearly PV)": "Kostenloses Abregelungsbudget (CH, 3 % der PV-Jahresenergie)", "Fault injection": "Fehler einspeisen",
    "Meter offline": "Zähler offline", "Charger 2 offline": "Ladepunkt 2 offline", "Heat pump offline": "Wärmepumpe offline", "Inverter 1 offline": "Wechselrichter 1 offline", "Battery offline": "Batterie offline",
    "The device stops answering Modbus. Watch the fallbacks: a charger drops to its own 6 A failsafe and the battery goes idle after 30 s; without the meter the budget shrinks to the floor.": "Das Gerät antwortet nicht mehr per Modbus. Beobachten Sie die Rückfallebenen: ein Ladepunkt fällt selbst auf 6 A zurück, die Batterie geht nach 30 s in den Ruhezustand; ohne Zähler schrumpft das Budget auf die Mindestleistung.",
    "Field devices (Modbus TCP) — setpoint written by the gateway → measured": "Feldgeräte (Modbus TCP) — vom Gateway geschriebener Sollwert → gemessen",
    "IEC 60870-5-104 frames on the DSO link": "IEC-60870-5-104-Telegramme zur Netzleitstelle", "show raw bytes": "Rohdaten zeigen",
  },
};

// Keys used by data-i18n / data-i18n-html in the calculator section.
export const KEYS = {
  en: {
    cTitle: "Feeder calculator · which way of ending a reduction is most viable?",
    cIntro: "When a §14a reduction ends at the same time for many sites, their chargers and heat pumps catch up at once. The research behind this project (20 sites × 10 days × 33 cases, <a href=\"https://github.com/maycu-byte/grid-edge-gateway/tree/main/docs/study/rebound\">docs/study/rebound</a>) found that a slow restart makes that rebound <i>gentler</i> but not <i>smaller</i>, and that a site planning by price avoids most of it. Test any combination yourself: the page simulates every site with the gateway's own code and compares it with the same evening without a reduction.",
    cSites: "Sites on the feeder", cSitesSub: "each is the demo depot with its own weather", cCap: "Transformer capacity per site",
    cFleet: "When the electric vans arrive", cFleetSub: "all at the same time = worst case for the transformer", cSame: "All at the same time", cMixed: "Varied times",
    cDay: "Day", winterDay: "Winter · 20 Jan 2025", springDay: "Spring · 6 Apr 2025", cDur: "Reduction from 17:30",
    cRelease: "How each site comes back", rStep: "At once", rRamp: "5-min ramp (DE rule)", rWait10: "Ramp + wait ≤10 min", rWait30: "Ramp + wait ≤30 min", rRamp30: "30-min ramp",
    cGroups: "Grid operator releases", cGroupsSub: "groups 15 min apart", gAll: "All at once", g2: "2 groups", g4: "4 groups",
    cCtl: "Site controller", cCtlSub: "the planner schedules against day-ahead prices; slower", ctlRules: "Rules only", ctlMpc: "Planner (MPC)",
    cRun: "Calculate", cAll: "Compare all options",
    cEmpty: "Pick a feeder on the left and press Calculate. Each site is simulated from 06:00 with the gateway's code; the evening then plays back here, minute by minute.",
    lgOption: "Feeder load, this option", lgBase: "Same evening, no reduction", lgCap: "Transformer capacity",
    cGridKey: "Each square is one site: green below its share of the transformer, amber near it, red above, blue exporting.",
    cCompare: "Options calculated so far", cCompareKey: "★ = most viable: nothing short for the vans, fewest minutes over the transformer after the release, then the lowest peak. Click a row to replay it.",
  },
  pt: {
    cTitle: "Calculadora do alimentador · qual forma de encerrar uma redução é a mais viável?",
    cIntro: "Quando uma redução do §14a termina ao mesmo tempo para muitos locais, os carregadores e as bombas de calor recuperam o atraso todos juntos. A pesquisa por trás deste projeto (20 locais × 10 dias × 33 casos, <a href=\"https://github.com/maycu-byte/grid-edge-gateway/tree/main/docs/study/rebound\">docs/study/rebound</a>) mostrou que religar devagar deixa esse retorno <i>mais suave</i>, mas não <i>menor</i>, e que um local que planeja pelo preço evita a maior parte dele. Teste qualquer combinação: a página simula cada local com o próprio código do gateway e compara com a mesma noite sem redução.",
    cSites: "Locais no alimentador", cSitesSub: "cada um é o depósito da demonstração, com seu próprio clima", cCap: "Capacidade do transformador por local",
    cFleet: "Chegada das vans elétricas", cFleetSub: "todas no mesmo horário = pior caso para o transformador", cSame: "Todas no mesmo horário", cMixed: "Horários variados",
    cDay: "Dia", winterDay: "Inverno · 20 jan 2025", springDay: "Primavera · 6 abr 2025", cDur: "Redução a partir das 17:30",
    cRelease: "Como cada local volta", rStep: "De uma vez", rRamp: "Rampa de 5 min (regra alemã)", rWait10: "Rampa + espera ≤10 min", rWait30: "Rampa + espera ≤30 min", rRamp30: "Rampa de 30 min",
    cGroups: "A distribuidora libera", cGroupsSub: "grupos com 15 min entre eles", gAll: "Todos juntos", g2: "2 grupos", g4: "4 grupos",
    cCtl: "Controle do local", cCtlSub: "o planejador organiza pelo preço do dia seguinte; mais lento", ctlRules: "Só regras", ctlMpc: "Planejador (MPC)",
    cRun: "Calcular", cAll: "Comparar todas as opções",
    cEmpty: "Escolha um alimentador à esquerda e clique em Calcular. Cada local é simulado desde as 06:00 com o código do gateway; depois a noite é reproduzida aqui, minuto a minuto.",
    lgOption: "Carga do alimentador, esta opção", lgBase: "Mesma noite, sem redução", lgCap: "Capacidade do transformador",
    cGridKey: "Cada quadrado é um local: verde abaixo da sua parte do transformador, amarelo perto dela, vermelho acima, azul exportando.",
    cCompare: "Opções calculadas até agora", cCompareKey: "★ = mais viável: nada faltando nas vans, menos minutos acima do transformador depois da liberação e, em seguida, o menor pico. Clique numa linha para reproduzi-la.",
  },
  de: {
    cTitle: "Strang-Rechner · wie beendet man eine Reduzierung am besten?",
    cIntro: "Endet eine §14a-Reduzierung für viele Standorte gleichzeitig, holen ihre Ladepunkte und Wärmepumpen alles auf einmal nach. Die Untersuchung hinter diesem Projekt (20 Standorte × 10 Tage × 33 Fälle, <a href=\"https://github.com/maycu-byte/grid-edge-gateway/tree/main/docs/study/rebound\">docs/study/rebound</a>) zeigt: ein langsamer Wiederanlauf macht diesen Nachholeffekt <i>sanfter</i>, aber nicht <i>kleiner</i>, und ein Standort, der nach Preis plant, vermeidet ihn größtenteils. Testen Sie jede Kombination: die Seite simuliert jeden Standort mit dem Code des Gateways und vergleicht mit demselben Abend ohne Reduzierung.",
    cSites: "Standorte am Strang", cSitesSub: "jeder ist das Demo-Depot mit eigenem Wetter", cCap: "Trafoleistung je Standort",
    cFleet: "Ankunft der E-Lieferwagen", cFleetSub: "alle gleichzeitig = ungünstigster Fall für den Trafo", cSame: "Alle gleichzeitig", cMixed: "Unterschiedlich",
    cDay: "Tag", winterDay: "Winter · 20. Jan. 2025", springDay: "Frühling · 6. Apr. 2025", cDur: "Reduzierung ab 17:30",
    cRelease: "Wie jeder Standort zurückkehrt", rStep: "Sofort", rRamp: "5-min-Rampe (DE-Regel)", rWait10: "Rampe + Wartezeit ≤10 min", rWait30: "Rampe + Wartezeit ≤30 min", rRamp30: "30-min-Rampe",
    cGroups: "Netzbetreiber gibt frei", cGroupsSub: "Gruppen im Abstand von 15 min", gAll: "Alle zugleich", g2: "2 Gruppen", g4: "4 Gruppen",
    cCtl: "Standortregelung", cCtlSub: "der Planer plant nach Day-Ahead-Preisen; langsamer", ctlRules: "Nur Regeln", ctlMpc: "Planer (MPC)",
    cRun: "Berechnen", cAll: "Alle Optionen vergleichen",
    cEmpty: "Wählen Sie links einen Strang und klicken Sie auf Berechnen. Jeder Standort wird ab 06:00 mit dem Gateway-Code simuliert; der Abend läuft dann hier Minute für Minute ab.",
    lgOption: "Strangleistung, diese Option", lgBase: "Derselbe Abend ohne Reduzierung", lgCap: "Trafoleistung",
    cGridKey: "Jedes Quadrat ist ein Standort: grün unter seinem Anteil am Trafo, gelb nahe daran, rot darüber, blau bei Einspeisung.",
    cCompare: "Bisher berechnete Optionen", cCompareKey: "★ = am besten geeignet: keinem Transporter fehlt Energie, die wenigsten Minuten über der Trafoleistung nach der Freigabe, dann die niedrigste Spitze. Klicken Sie auf eine Zeile, um sie abzuspielen.",
  },
};

const n1 = (v) => v;
export const T = {
  en: {
    country: {
      DE: { dim: "§14a EnWG · C_SC_NA_1 · IOA 5001", floor: "Pmin,14a = 0.4 × 14 kW heat pump + 5 × 0.6 × 4.2 kW (4 chargers + battery) = <b class=\"mono\">18.2 kW</b> (BNetzA BK6-22-300). PV surplus and battery discharge may be used on top.", feed: "No standing cap for this site; the DSO sends setpoints.", dimTitle: "§14a dimming active" },
      AT: { dim: "flexibility contract · C_SC_NA_1 · IOA 5001", floor: "No statutory minimum like §14a: this depot's contract keeps <b class=\"mono\">10 kW</b> and lets the DSO dim at most 2 hours a day.", feed: "ElWG Spitzenkappung: the DSO capped feed-in of this new PV system at <b>70%</b> of module peak power, always in force.", dimTitle: "Contract dimming active" },
      CH: { dim: "flexibility contract · C_SC_NA_1 · IOA 5001", floor: "Contract: <b class=\"mono\">8 kW</b> minimum, at most 3 hours a day. The owner forbade the DSO to use the heat pump (StromVV Art. 19d), so it is never limited.", feed: "The DSO may curtail at most <b>3%</b> of the yearly PV energy for free (StromVV Art. 19c); beyond that only in an emergency. Late-year scenario: 3,400 of 3,420 kWh already used.", dimTitle: "Contract dimming active" },
    },
    strategy: {
      rules: "Rules only: the battery maximises self-consumption, cars charge at full power as they arrive, the heat pump follows its own thermostat. The safety layer applies the DSO's limits.",
      mpc: "MPC: every 15 minutes (and when a car arrives) a 24-hour convex QP plans battery, cars and heat pump against day-ahead prices, a demand charge on the peak quarter-hour, departure times, comfort and battery ageing. The safety layer follows the plan only as far as the rules allow.",
      "mpc-cc": "MPC with chance constraints: as MPC, but in and just before an expected dimming the battery keeps a reserve against PV and load forecast errors (ε = 5%).",
    },
    normal: "Normal operation", feedInForce: (p, k) => `Feed-in limit in force: ${p}%. Export held at ${k} kW; the depot and the battery use the rest of the PV.`,
    noLimitRules: "No DSO limit in force. Chargers run at full power, the battery covers imports, the heat pump follows its thermostat.",
    noLimitPlan: "No DSO limit in force. The site follows the plan: cars, battery and heat pump are scheduled against prices, departures and comfort.",
    dimmedText: (b, f) => `Controllable devices may draw ${b} from the grid: the ${f} floor plus PV surplus, minus a 0.3 kW margin. The battery discharges on top.`,
    releasing: "Gradual release", releasingText: "The dimming ended. Power returns over 5 minutes so the feeder does not see a step.",
    emergency: "Emergency: day limits, budgets and opt-outs do not apply.", refused: (r) => `Refused: ${r}.`, fallback: (l) => `Fallback: ${l}`,
    gUnknown: "unknown (meter offline)", gNow: (v) => `now ${v}`, gNoLimit: "no limit", gLimit: (v) => `limit ${v}`,
    bCurtailed: (v) => `${v} kWh curtailed this year`, bBudget: (v) => `budget ${v} kWh`,
    sMeterOff: "grid · meter offline", sImport: "importing from grid", sExport: "exporting to grid",
    sPrice: (ct) => `day-ahead · you pay ${ct} ct/kWh`, sIndoor: (min, out) => `indoor · at least ${min} °C now · ${out} °C outside`, sPv: (a, l) => `PV · ${a} kW available · limit ${l}%`,
    vRows: ["Energy", "Battery ageing", "Peak charge", "Peak, 15 min", "Total", "Below comfort", "EV energy missing"], vHead: ["this site", "rules only"],
    vBoth: "Both copies run on rules; switch to MPC to compare.",
    vSaved: (e) => `Saved <b class="mono">${e}</b> so far against the same site on rules alone, same weather and commands.`,
    vBehind: (e, ahead) => `Behind by <b class="mono">${e}</b> so far against the same site on rules alone${ahead ? `: it has bought ahead of a price peak — ${ahead} than on rules. The saving comes when that energy is used.` : "."}`,
    vBattery: (k) => `its battery holds ${k} kWh more`, vWarmer: (k) => `the building is ${k} K warmer`, vAnd: " and ",
    vSolve: (n, ms) => `${n} plans, ${ms} ms per solve (96 × 15-min QP, in your browser)`,
    dCharger: "Charger", dHp: "Heat pump", dBattery: "Battery", dNoCar: "no car", dLeaves: (t) => `leaves in ${t}`, dInside: (c) => `${c} °C inside · 14 kW rated`, dOptOut: " · opted out", dSoc: (s) => `${s}% of 100 kWh · ±50 kW`,
    st: { available: "available", waiting: "waiting", charging: "charging", failsafe: "failsafe", finished: "finished", offline: "offline", planned: "planned", limited: "limited", thermostat: "thermostat", idle: "idle", discharging: "discharging" },
    cDimming: "dimming", cFeedin: "feed-in limit", cGrid: "grid", cPv: "PV", cLoads: "loads", cBattery: "battery",
    tGrid: "grid", tPv: "PV", tLoads: "chargers + heat pump", tBattery: "battery", tIndoor: "indoor", tBudget: "dimming budget", tExport: "export limit",
    dsoToSite: "DSO → site", siteToDso: "site → DSO",
    // calculator
    rel: { step: "At once", ramp: "5-min ramp", wait10: "Ramp + wait ≤10 min", wait30: "Ramp + wait ≤30 min", ramp30: "30-min ramp" },
    ctl: { rules: "Rules", mpc: "Planner" }, groups: (n) => `${n} groups`,
    capSub: (t, n) => `total ${t} kW for ${n} site${n > 1 ? "s" : ""}`,
    noRed: (c) => `No reduction, ${c}`, siteOf: (l, i, n) => `${l}: site ${i} of ${n}`,
    done: (o, s) => `Done: ${o} option${o > 1 ? "s" : ""}, ${s} site-evenings simulated.`,
    kPeak: (p) => `peak after the release · ${p} % of the transformer`,
    kOver: (a, all, base) => `over the transformer after the release · ${all} min over 16:30–22:00 (without a reduction: ${base} min)`,
    kRebound: "rebound: above the same evening without a reduction", kPushed: "energy pushed past the reduction", kRise: "steepest rise of the feeder load",
    kShort: (ps) => `short for the vans at departure (${ps} per site)`,
    vOverload: (m) => `the transformer is overloaded for ${m} minutes after the release`, vShort: (k) => `vans leave ${k} kWh short`, vJoin: ", and ",
    vOk: "the feeder stays within the transformer after the release and every van leaves charged.",
    tHead: ["Option", "Peak after, kW", "Min over transformer", "Rebound, kW", "Energy pushed, kWh", "Steepest rise, kW/min", "Vans short, kWh", "Energy cost 16:30–22:00"],
    aBefore: "before the reduction", aDim: "reduction in force", aRel: "being released", aAfter: "after the reduction", aOver: "over the transformer",
    aNow: (v, p) => `feeder now ${v} · ${p} % of the transformer`, aCap: (v) => `transformer ${v}`,
    reductionLbl: "reduction", transformerLbl: (v) => `transformer ${v}`,
  },
};

T.pt = {
  ...T.en,
  country: {
    DE: { dim: "§14a EnWG · C_SC_NA_1 · IOA 5001", floor: "Pmin,14a = 0,4 × 14 kW da bomba de calor + 5 × 0,6 × 4,2 kW (4 carregadores + bateria) = <b class=\"mono\">18,2 kW</b> (BNetzA BK6-22-300). A sobra solar e a descarga da bateria podem ser usadas por cima.", feed: "Sem limite fixo para este local; a distribuidora envia valores de referência.", dimTitle: "Redução do §14a ativa" },
    AT: { dim: "contrato de flexibilidade · C_SC_NA_1 · IOA 5001", floor: "Não há mínimo legal como o §14a: o contrato deste depósito garante <b class=\"mono\">10 kW</b> e permite reduzir no máximo 2 horas por dia.", feed: "Spitzenkappung do ElWG: a distribuidora limitou a injeção deste sistema solar novo a <b>70%</b> da potência de pico, sempre em vigor.", dimTitle: "Redução por contrato ativa" },
    CH: { dim: "contrato de flexibilidade · C_SC_NA_1 · IOA 5001", floor: "Contrato: mínimo de <b class=\"mono\">8 kW</b>, no máximo 3 horas por dia. O dono proibiu o uso da bomba de calor (StromVV art. 19d), então ela nunca é limitada.", feed: "A distribuidora pode cortar de graça no máximo <b>3%</b> da energia solar do ano (StromVV art. 19c); além disso, só em emergência. Cenário de fim de ano: 3.400 de 3.420 kWh já usados.", dimTitle: "Redução por contrato ativa" },
  },
  strategy: {
    rules: "Só regras: a bateria maximiza o autoconsumo, os carros carregam na potência máxima ao chegar e a bomba de calor segue o próprio termostato. A camada de segurança aplica os limites da distribuidora.",
    mpc: "MPC: a cada 15 minutos (e quando chega um carro) um programa quadrático convexo de 24 horas organiza bateria, carros e bomba de calor pelos preços do dia seguinte, a tarifa de demanda, os horários de saída, o conforto e o desgaste da bateria. A camada de segurança só segue o plano até onde as regras permitem.",
    "mpc-cc": "MPC probabilístico: como o MPC, mas durante e pouco antes de uma redução esperada a bateria guarda uma reserva contra erros de previsão solar e de consumo (ε = 5%).",
  },
  normal: "Funcionamento normal", feedInForce: (p, k) => `Limite de injeção em vigor: ${p}%. Exportação mantida em ${k} kW; o depósito e a bateria usam o resto da energia solar.`,
  noLimitRules: "Nenhum limite da distribuidora. Os carregadores funcionam na potência máxima, a bateria cobre a compra e a bomba de calor segue o termostato.",
  noLimitPlan: "Nenhum limite da distribuidora. O local segue o plano: carros, bateria e bomba de calor são organizados por preço, horários de saída e conforto.",
  dimmedText: (b, f) => `Os equipamentos controláveis podem puxar ${b} da rede: o piso de ${f} mais a sobra solar, menos uma margem de 0,3 kW. A bateria descarrega por cima.`,
  releasing: "Retorno gradual", releasingText: "A redução terminou. A potência volta ao longo de 5 minutos para o alimentador não ver um degrau.",
  emergency: "Emergência: limites diários, orçamentos e recusas não se aplicam.", refused: (r) => `Recusado: ${r}.`, fallback: (l) => `Modo de segurança: ${l}`,
  gUnknown: "desconhecido (medidor fora do ar)", gNow: (v) => `agora ${v}`, gNoLimit: "sem limite", gLimit: (v) => `limite ${v}`,
  bCurtailed: (v) => `${v} kWh cortados neste ano`, bBudget: (v) => `orçamento ${v} kWh`,
  sMeterOff: "rede · medidor fora do ar", sImport: "comprando da rede", sExport: "vendendo para a rede",
  sPrice: (ct) => `dia seguinte · você paga ${ct} ct/kWh`, sIndoor: (min, out) => `interna · no mínimo ${min} °C agora · ${out} °C lá fora`, sPv: (a, l) => `solar · ${a} kW disponíveis · limite ${l}%`,
  vRows: ["Energia", "Desgaste da bateria", "Tarifa de demanda", "Pico, 15 min", "Total", "Abaixo do conforto", "Energia faltando nas vans"], vHead: ["este local", "só regras"],
  vBoth: "As duas cópias usam só regras; troque para MPC para comparar.",
  vSaved: (e) => `Economizou <b class="mono">${e}</b> até agora contra o mesmo local só com regras, mesmo clima e mesmos comandos.`,
  vBehind: (e, ahead) => `Atrás por <b class="mono">${e}</b> até agora contra o mesmo local só com regras${ahead ? `: comprou antes de um pico de preço — ${ahead} do que com regras. A economia vem quando essa energia for usada.` : "."}`,
  vBattery: (k) => `a bateria tem ${k} kWh a mais`, vWarmer: (k) => `o prédio está ${k} K mais quente`, vAnd: " e ",
  vSolve: (n, ms) => `${n} planos, ${ms} ms por solução (QP de 96 × 15 min, no seu navegador)`,
  dCharger: "Carregador", dHp: "Bomba de calor", dBattery: "Bateria", dNoCar: "sem carro", dLeaves: (t) => `sai em ${t}`, dInside: (c) => `${c} °C dentro · 14 kW nominal`, dOptOut: " · recusada", dSoc: (s) => `${s}% de 100 kWh · ±50 kW`,
  st: { available: "livre", waiting: "esperando", charging: "carregando", failsafe: "segurança", finished: "completo", offline: "fora do ar", planned: "planejada", limited: "limitada", thermostat: "termostato", idle: "parada", discharging: "descarregando" },
  cDimming: "redução", cFeedin: "limite de injeção", cGrid: "rede", cPv: "solar", cLoads: "cargas", cBattery: "bateria",
  tGrid: "rede", tPv: "solar", tLoads: "carregadores + bomba de calor", tBattery: "bateria", tIndoor: "interna", tBudget: "limite da redução", tExport: "limite de exportação",
  dsoToSite: "distribuidora → local", siteToDso: "local → distribuidora",
  rel: { step: "De uma vez", ramp: "Rampa de 5 min", wait10: "Rampa + espera ≤10 min", wait30: "Rampa + espera ≤30 min", ramp30: "Rampa de 30 min" },
  ctl: { rules: "Regras", mpc: "Planejador" }, groups: (n) => `${n} grupos`,
  capSub: (t, n) => `total de ${t} kW para ${n} ${n > 1 ? "locais" : "local"}`,
  noRed: (c) => `Sem redução, ${c}`, siteOf: (l, i, n) => `${l}: local ${i} de ${n}`,
  done: (o, s) => `Pronto: ${o} ${o > 1 ? "opções" : "opção"}, ${s} noites de locais simuladas.`,
  kPeak: (p) => `pico depois da liberação · ${p} % do transformador`,
  kOver: (a, all, base) => `acima do transformador depois da liberação · ${all} min entre 16:30 e 22:00 (sem redução: ${base} min)`,
  kRebound: "retorno: acima da mesma noite sem redução", kPushed: "energia empurrada para depois da redução", kRise: "subida mais íngreme da carga",
  kShort: (ps) => `faltando nas vans na saída (${ps} por local)`,
  vOverload: (m) => `o transformador fica sobrecarregado por ${m} minutos depois da liberação`, vShort: (k) => `as vans saem com ${k} kWh faltando`, vJoin: " e ",
  vOk: "o alimentador fica dentro do transformador depois da liberação e todas as vans saem carregadas.",
  tHead: ["Opção", "Pico depois, kW", "Min acima do transformador", "Retorno, kW", "Energia empurrada, kWh", "Subida máx., kW/min", "Faltando nas vans, kWh", "Custo de energia 16:30–22:00"],
  aBefore: "antes da redução", aDim: "redução em vigor", aRel: "sendo liberado", aAfter: "depois da redução", aOver: "acima do transformador",
  aNow: (v, p) => `alimentador agora ${v} · ${p} % do transformador`, aCap: (v) => `transformador ${v}`,
  reductionLbl: "redução", transformerLbl: (v) => `transformador ${v}`,
};

T.de = {
  ...T.en,
  country: {
    DE: { dim: "§14a EnWG · C_SC_NA_1 · IOA 5001", floor: "Pmin,14a = 0,4 × 14 kW Wärmepumpe + 5 × 0,6 × 4,2 kW (4 Ladepunkte + Batterie) = <b class=\"mono\">18,2 kW</b> (BNetzA BK6-22-300). PV-Überschuss und Batterieentladung dürfen zusätzlich genutzt werden.", feed: "Keine feste Grenze für diesen Standort; der Netzbetreiber sendet Sollwerte.", dimTitle: "§14a-Dimmung aktiv" },
    AT: { dim: "Flexibilitätsvertrag · C_SC_NA_1 · IOA 5001", floor: "Keine gesetzliche Mindestleistung wie §14a: der Vertrag dieses Depots sichert <b class=\"mono\">10 kW</b> und erlaubt höchstens 2 Stunden Dimmung pro Tag.", feed: "ElWG-Spitzenkappung: der Netzbetreiber hat die Einspeisung dieser neuen PV-Anlage auf <b>70 %</b> der Modulspitzenleistung begrenzt, dauerhaft.", dimTitle: "Vertragliche Dimmung aktiv" },
    CH: { dim: "Flexibilitätsvertrag · C_SC_NA_1 · IOA 5001", floor: "Vertrag: mindestens <b class=\"mono\">8 kW</b>, höchstens 3 Stunden pro Tag. Der Eigentümer hat die Nutzung der Wärmepumpe untersagt (StromVV Art. 19d), sie wird also nie begrenzt.", feed: "Der Netzbetreiber darf höchstens <b>3 %</b> der PV-Jahresenergie kostenlos abregeln (StromVV Art. 19c); darüber hinaus nur im Notfall. Szenario zum Jahresende: 3.400 von 3.420 kWh bereits verbraucht.", dimTitle: "Vertragliche Dimmung aktiv" },
  },
  strategy: {
    rules: "Nur Regeln: die Batterie maximiert den Eigenverbrauch, Fahrzeuge laden bei Ankunft mit voller Leistung, die Wärmepumpe folgt ihrem eigenen Thermostat. Die Sicherheitsschicht setzt die Grenzen des Netzbetreibers um.",
    mpc: "MPC: alle 15 Minuten (und bei Ankunft eines Fahrzeugs) plant ein konvexes 24-Stunden-QP Batterie, Fahrzeuge und Wärmepumpe nach Day-Ahead-Preisen, Leistungspreis auf die höchste Viertelstunde, Abfahrtszeiten, Komfort und Batteriealterung. Die Sicherheitsschicht folgt dem Plan nur, soweit die Regeln es erlauben.",
    "mpc-cc": "MPC mit Chance Constraints: wie MPC, aber während und kurz vor einer erwarteten Dimmung hält die Batterie eine Reserve gegen Prognosefehler bei PV und Last (ε = 5 %).",
  },
  normal: "Normalbetrieb", feedInForce: (p, k) => `Einspeisegrenze aktiv: ${p} %. Einspeisung auf ${k} kW gehalten; Depot und Batterie nutzen den Rest der PV-Leistung.`,
  noLimitRules: "Keine Vorgabe des Netzbetreibers. Die Ladepunkte laufen mit voller Leistung, die Batterie deckt den Bezug, die Wärmepumpe folgt ihrem Thermostat.",
  noLimitPlan: "Keine Vorgabe des Netzbetreibers. Der Standort folgt dem Plan: Fahrzeuge, Batterie und Wärmepumpe werden nach Preisen, Abfahrtszeiten und Komfort geplant.",
  dimmedText: (b, f) => `Die steuerbaren Verbraucher dürfen ${b} aus dem Netz beziehen: die Mindestleistung von ${f} plus PV-Überschuss, abzüglich 0,3 kW Reserve. Die Batterie entlädt zusätzlich.`,
  releasing: "Schrittweise Freigabe", releasingText: "Die Dimmung ist beendet. Die Leistung kehrt über 5 Minuten zurück, damit der Strang keinen Sprung sieht.",
  emergency: "Notfall: Tageslimits, Budgets und Widersprüche gelten nicht.", refused: (r) => `Abgelehnt: ${r}.`, fallback: (l) => `Rückfallebene: ${l}`,
  gUnknown: "unbekannt (Zähler offline)", gNow: (v) => `jetzt ${v}`, gNoLimit: "keine Grenze", gLimit: (v) => `Grenze ${v}`,
  bCurtailed: (v) => `${v} kWh in diesem Jahr abgeregelt`, bBudget: (v) => `Budget ${v} kWh`,
  sMeterOff: "Netz · Zähler offline", sImport: "Bezug aus dem Netz", sExport: "Einspeisung ins Netz",
  sPrice: (ct) => `Day-Ahead · Sie zahlen ${ct} ct/kWh`, sIndoor: (min, out) => `innen · jetzt mindestens ${min} °C · ${out} °C außen`, sPv: (a, l) => `PV · ${a} kW verfügbar · Grenze ${l} %`,
  vRows: ["Energie", "Batteriealterung", "Leistungspreis", "Spitze, 15 min", "Summe", "Unter Komfort", "Fehlende Ladeenergie"], vHead: ["dieser Standort", "nur Regeln"],
  vBoth: "Beide Kopien laufen nur mit Regeln; wechseln Sie zu MPC für den Vergleich.",
  vSaved: (e) => `Bisher <b class="mono">${e}</b> gespart gegenüber demselben Standort nur mit Regeln, bei gleichem Wetter und gleichen Befehlen.`,
  vBehind: (e, ahead) => `Bisher <b class="mono">${e}</b> im Rückstand gegenüber demselben Standort nur mit Regeln${ahead ? `: er hat vor einer Preisspitze eingekauft — ${ahead} als mit Regeln. Die Ersparnis kommt, wenn diese Energie genutzt wird.` : "."}`,
  vBattery: (k) => `seine Batterie hält ${k} kWh mehr`, vWarmer: (k) => `das Gebäude ist ${k} K wärmer`, vAnd: " und ",
  vSolve: (n, ms) => `${n} Pläne, ${ms} ms pro Lösung (QP mit 96 × 15 min, in Ihrem Browser)`,
  dCharger: "Ladepunkt", dHp: "Wärmepumpe", dBattery: "Batterie", dNoCar: "kein Fahrzeug", dLeaves: (t) => `fährt in ${t}`, dInside: (c) => `${c} °C innen · 14 kW Nennleistung`, dOptOut: " · widersprochen", dSoc: (s) => `${s} % von 100 kWh · ±50 kW`,
  st: { available: "frei", waiting: "wartet", charging: "lädt", failsafe: "Rückfall", finished: "fertig", offline: "offline", planned: "geplant", limited: "begrenzt", thermostat: "Thermostat", idle: "Ruhe", discharging: "entlädt" },
  cDimming: "Dimmung", cFeedin: "Einspeisegrenze", cGrid: "Netz", cPv: "PV", cLoads: "Lasten", cBattery: "Batterie",
  tGrid: "Netz", tPv: "PV", tLoads: "Ladepunkte + Wärmepumpe", tBattery: "Batterie", tIndoor: "innen", tBudget: "Dimmbudget", tExport: "Einspeisegrenze",
  dsoToSite: "Netzbetreiber → Standort", siteToDso: "Standort → Netzbetreiber",
  rel: { step: "Sofort", ramp: "5-min-Rampe", wait10: "Rampe + Wartezeit ≤10 min", wait30: "Rampe + Wartezeit ≤30 min", ramp30: "30-min-Rampe" },
  ctl: { rules: "Regeln", mpc: "Planer" }, groups: (n) => `${n} Gruppen`,
  capSub: (t, n) => `insgesamt ${t} kW für ${n} ${n > 1 ? "Standorte" : "Standort"}`,
  noRed: (c) => `Ohne Reduzierung, ${c}`, siteOf: (l, i, n) => `${l}: Standort ${i} von ${n}`,
  done: (o, s) => `Fertig: ${o} ${o > 1 ? "Optionen" : "Option"}, ${s} Standort-Abende simuliert.`,
  kPeak: (p) => `Spitze nach der Freigabe · ${p} % der Trafoleistung`,
  kOver: (a, all, base) => `über der Trafoleistung nach der Freigabe · ${all} min zwischen 16:30 und 22:00 (ohne Reduzierung: ${base} min)`,
  kRebound: "Nachholeffekt: über demselben Abend ohne Reduzierung", kPushed: "nach hinten verschobene Energie", kRise: "steilster Anstieg der Strangleistung",
  kShort: (ps) => `fehlt den Transportern bei Abfahrt (${ps} je Standort)`,
  vOverload: (m) => `der Trafo ist nach der Freigabe ${m} Minuten überlastet`, vShort: (k) => `den Transportern fehlen ${k} kWh`, vJoin: " und ",
  vOk: "der Strang bleibt nach der Freigabe unter der Trafoleistung und jeder Transporter fährt geladen ab.",
  tHead: ["Option", "Spitze danach, kW", "Min über Trafo", "Nachholeffekt, kW", "Verschobene Energie, kWh", "Steilster Anstieg, kW/min", "Fehlende Ladeenergie, kWh", "Energiekosten 16:30–22:00"],
  aBefore: "vor der Reduzierung", aDim: "Reduzierung aktiv", aRel: "wird freigegeben", aAfter: "nach der Reduzierung", aOver: "über der Trafoleistung",
  aNow: (v, p) => `Strang jetzt ${v} · ${p} % der Trafoleistung`, aCap: (v) => `Trafo ${v}`,
  reductionLbl: "Reduzierung", transformerLbl: (v) => `Trafo ${v}`,
};
void n1;

// Real extreme days of 2025 (buttons of the demo and the calculator).
const EXTREMES = {
  pt: { "Dunkelflaute · 20 Jan": "Dunkelflaute · 20 jan", "Negative noon · 11 May": "Meio-dia negativo · 11 mai", "Coldest day · 22 Nov": "Dia mais frio · 22 nov", "Heatwave · 1 Jul": "Onda de calor · 1 jul",
    "the real extremes of 2025: 583 €/MWh on 20 Jan at 17:00, −250 €/MWh on 11 May at 13:00, −4.2 °C on 22 Nov, 33.5 °C and 476 €/MWh on 1 Jul (SMARD prices, measured weather)": "os extremos reais de 2025: 583 €/MWh em 20 jan às 17:00, −250 €/MWh em 11 mai às 13:00, −4,2 °C em 22 nov, 33,5 °C e 476 €/MWh em 1 jul (preços do SMARD, clima medido)",
    "2025: highest 583 on 20 Jan at 17:00 · lowest −250 on 11 May at 13:00": "2025: máximo de 583 em 20 jan às 17:00 · mínimo de −250 em 11 mai às 13:00" },
  de: { "Dunkelflaute · 20 Jan": "Dunkelflaute · 20. Jan.", "Negative noon · 11 May": "Negativer Mittag · 11. Mai", "Coldest day · 22 Nov": "Kältester Tag · 22. Nov.", "Heatwave · 1 Jul": "Hitzewelle · 1. Juli",
    "the real extremes of 2025: 583 €/MWh on 20 Jan at 17:00, −250 €/MWh on 11 May at 13:00, −4.2 °C on 22 Nov, 33.5 °C and 476 €/MWh on 1 Jul (SMARD prices, measured weather)": "die echten Extreme 2025: 583 €/MWh am 20. Jan. um 17:00, −250 €/MWh am 11. Mai um 13:00, −4,2 °C am 22. Nov., 33,5 °C und 476 €/MWh am 1. Juli (SMARD-Preise, gemessenes Wetter)",
    "2025: highest 583 on 20 Jan at 17:00 · lowest −250 on 11 May at 13:00": "2025: Höchstwert 583 am 20. Jan. um 17:00 · Tiefstwert −250 am 11. Mai um 13:00" },
};
Object.assign(UI.pt, EXTREMES.pt);
Object.assign(UI.de, EXTREMES.de);
Object.assign(KEYS.en, { xDunkel: "Dunkelflaute · 20 Jan", xNeg: "Negative noon · 11 May", xCold: "Coldest day · 22 Nov", xHeat: "Heatwave · 1 Jul" });
Object.assign(KEYS.pt, { xDunkel: EXTREMES.pt["Dunkelflaute · 20 Jan"], xNeg: EXTREMES.pt["Negative noon · 11 May"], xCold: EXTREMES.pt["Coldest day · 22 Nov"], xHeat: EXTREMES.pt["Heatwave · 1 Jul"] });
Object.assign(KEYS.de, { xDunkel: EXTREMES.de["Dunkelflaute · 20 Jan"], xNeg: EXTREMES.de["Negative noon · 11 May"], xCold: EXTREMES.de["Coldest day · 22 Nov"], xHeat: EXTREMES.de["Heatwave · 1 Jul"] });

// The year view (year.js).
Object.assign(T.en, {
  yMonths: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
  yClass: { k0: "no overload, nothing to do", k1: "would overload; the reduction solves it", k2: "the rebound overloads it again after 19:30", k3: "overloads even with the reduction" },
  yNeed: (c) => `evenings the feeder would overload at ${c} kW per depot, so the grid operator reduces`,
  yRebound: "of them, the rebound after the release overloads it again",
  yStill: "of them, it still overloads with the reduction",
  ySolved: "of them, the reduction solves it",
  yOther: (o) => `the same with ${o === "mpc" ? "the planner" : "rules only"}: evenings over · rebound evenings`,
  yTemp: "mean temperature, Stuttgart", ySun: "sun: what 1 kWp of PV could make", yPrice: "day-ahead price 17:00–20:00",
  yWith: "with the reduction 17:30–19:30", yWithout: "the same evening without it", lgCap: "Transformer capacity",
  yDayNote: (a, b, p) => `Vans left short, whole feeder: ${a} kWh without the reduction, ${b} kWh with it. Highest load after the release: ${p} kW per depot.`,
  yOpenCalc: "Open this evening in the calculator", yOpenDemo: "Play this day in the live demo",
  yPerDepot: "kW per depot", yFailed: "The year's results could not be loaded",
});
Object.assign(T.pt, {
  yMonths: ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"],
  yClass: { k0: "sem sobrecarga, nada a fazer", k1: "sobrecarregaria; a redução resolve", k2: "o retorno sobrecarrega de novo depois das 19:30", k3: "sobrecarrega mesmo com a redução" },
  yNeed: (c) => `noites em que o alimentador sobrecarregaria com ${c} kW por depósito, e a distribuidora reduz`,
  yRebound: "dessas, o retorno depois da liberação sobrecarrega de novo",
  yStill: "dessas, continua sobrecarregado mesmo com a redução",
  ySolved: "dessas, a redução resolve",
  yOther: (o) => `o mesmo ${o === "mpc" ? "com o planejador" : "só com regras"}: noites acima · noites com retorno`,
  yTemp: "temperatura média, Stuttgart", ySun: "sol: o que 1 kWp de painel produziria", yPrice: "preço do dia seguinte 17:00–20:00",
  yWith: "com a redução 17:30–19:30", yWithout: "a mesma noite sem ela", lgCap: "Capacidade do transformador",
  yDayNote: (a, b, p) => `Energia faltando nas vans, alimentador inteiro: ${a} kWh sem a redução, ${b} kWh com ela. Maior carga depois da liberação: ${p} kW por depósito.`,
  yOpenCalc: "Abrir esta noite na calculadora", yOpenDemo: "Rodar este dia na demonstração",
  yPerDepot: "kW por depósito", yFailed: "Não foi possível carregar os resultados do ano",
});
Object.assign(T.de, {
  yMonths: ["Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez"],
  yClass: { k0: "keine Überlast, nichts zu tun", k1: "wäre überlastet; die Reduzierung löst es", k2: "der Nachholeffekt überlastet nach 19:30 erneut", k3: "überlastet trotz Reduzierung" },
  yNeed: (c) => `Abende, an denen der Strang bei ${c} kW je Depot überlastet wäre und der Netzbetreiber reduziert`,
  yRebound: "davon überlastet der Nachholeffekt nach der Freigabe erneut",
  yStill: "davon bleibt er trotz Reduzierung überlastet",
  ySolved: "davon löst die Reduzierung es",
  yOther: (o) => `dasselbe ${o === "mpc" ? "mit dem Planer" : "nur mit Regeln"}: Abende über · Abende mit Nachholeffekt`,
  yTemp: "Mitteltemperatur, Stuttgart", ySun: "Sonne: was 1 kWp PV erzeugen könnte", yPrice: "Day-Ahead-Preis 17:00–20:00",
  yWith: "mit Reduzierung 17:30–19:30", yWithout: "derselbe Abend ohne", lgCap: "Trafoleistung",
  yDayNote: (a, b, p) => `Fehlende Ladeenergie, ganzer Strang: ${a} kWh ohne Reduzierung, ${b} kWh mit. Höchste Last nach der Freigabe: ${p} kW je Depot.`,
  yOpenCalc: "Diesen Abend im Rechner öffnen", yOpenDemo: "Diesen Tag in der Live-Demo abspielen",
  yPerDepot: "kW je Depot", yFailed: "Die Jahresergebnisse konnten nicht geladen werden",
});
Object.assign(KEYS.en, { orAnyDay: "or any day of 2025:" });
Object.assign(KEYS.pt, {
  orAnyDay: "ou qualquer dia de 2025:",
  yTitle: "2025, noite a noite · com que frequência o problema acontece?",
  yIntro: "Vinte depósitos no mesmo alimentador, simulados em <b>todas as noites de 2025</b> com os preços reais de energia e o clima medido de cada dia. A distribuidora só reduz o consumo nos dias em que o transformador sobrecarregaria. Cada quadrado é um dia: escolha o tamanho do transformador e o controle dos depósitos, depois clique num dia para ver a noite dele.",
  yCap: "Transformador por depósito", yCtl: "Os depósitos usam",
  yK0: "sem sobrecarga: nada a fazer", yK1: "sobrecarregaria: a redução resolve", yK2: "o retorno sobrecarrega de novo depois das 19:30", yK3: "sobrecarrega mesmo com a redução",
  yLoading: "Carregando o ano…",
});
Object.assign(KEYS.de, {
  orAnyDay: "oder ein beliebiger Tag 2025:",
  yTitle: "2025, Abend für Abend · wie oft tritt das Problem auf?",
  yIntro: "Zwanzig Depots an einem Strang, simuliert an <b>jedem Abend des Jahres 2025</b> mit den echten Strompreisen und dem gemessenen Wetter des Tages. Der Netzbetreiber reduziert nur an Tagen, an denen der Trafo sonst überlastet wäre. Jedes Quadrat ist ein Tag: Trafogröße und Standortregelung wählen, dann einen Tag anklicken, um seinen Abend zu sehen.",
  yCap: "Trafo je Depot", yCtl: "Die Depots laufen mit",
  yK0: "keine Überlast: nichts zu tun", yK1: "wäre überlastet: die Reduzierung löst es", yK2: "der Nachholeffekt überlastet nach 19:30 erneut", yK3: "überlastet trotz Reduzierung",
  yLoading: "Das Jahr wird geladen…",
});
Object.assign(UI.pt, { "or any day:": "ou qualquer dia:" });
Object.assign(UI.de, { "or any day:": "oder beliebiger Tag:" });

Object.assign(T.en, { restartDay: "The day is over: press to start it again" });
Object.assign(T.pt, { restartDay: "O dia terminou: clique para recomeçar" });
Object.assign(T.de, { restartDay: "Der Tag ist vorbei: klicken, um neu zu starten" });
Object.assign(T.en, { yCompare: (o, n, r) => o === "mpc"
  ? `<b>With the planner</b>, the same year: ${n} evenings over the transformer, ${r} of them overloaded again by the rebound.`
  : `<b>With rules only</b>, the same year: ${n} evenings over the transformer, ${r} of them overloaded again by the rebound.` });
Object.assign(T.pt, { yCompare: (o, n, r) => o === "mpc"
  ? `<b>Com o planejador</b>, o mesmo ano: ${n} noites acima do transformador, ${r} delas sobrecarregadas de novo pelo retorno.`
  : `<b>Só com regras</b>, o mesmo ano: ${n} noites acima do transformador, ${r} delas sobrecarregadas de novo pelo retorno.` });
Object.assign(T.de, { yCompare: (o, n, r) => o === "mpc"
  ? `<b>Mit dem Planer</b>, dasselbe Jahr: ${n} Abende über der Trafoleistung, ${r} davon erneut überlastet durch den Nachholeffekt.`
  : `<b>Nur mit Regeln</b>, dasselbe Jahr: ${n} Abende über der Trafoleistung, ${r} davon erneut überlastet durch den Nachholeffekt.` });
