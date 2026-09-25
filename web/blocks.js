// Long passages of the page in Portuguese and German. Each key is the
// data-block attribute of the element whose innerHTML is swapped; English
// is read from the page itself.

const GH = "https://github.com/maycu-byte/grid-edge-gateway";

export const BLOCKS = {
  pt: {
    intro: "Um controlador de local em Rust que fica entre a distribuidora de energia e um local comercial que produz e consome energia. Ele fala IEC 60870-5-104 com o centro de controle da distribuidora e SunSpec Modbus com os inversores, carregadores, bomba de calor e bateria. Um planejador preditivo (MPC) organiza o local pelos preços reais do dia seguinte; uma camada de segurança em tempo real aplica as regras de cada país: §14a EnWG na Alemanha, o limite de injeção do ElWG na Áustria, o orçamento de corte de 3% e os contratos de flexibilidade na Suíça. O que roda abaixo é esse código, compilado para WebAssembly.",
    howCols: `<div><h3>Do lado da distribuidora</h3><p>O gateway é uma estação controlada IEC 60870-5-104, o protocolo que as distribuidoras alemãs exigem para o telecontrole de instalações a partir de 100 kW. Os comandos são confirmados (ou recusados com confirmação negativa), as medições saem espontaneamente com carimbo de tempo, e o enlace usa TLS com certificados de cliente no espírito da IEC 62351-3. A pilha do protocolo foi escrita do zero e testada contra a lib60870.</p></div>
      <div><h3>Duas camadas de controle</h3><p>Um planejador (MPC) olha 24 horas à frente a cada 15 minutos e organiza bateria, carros e bomba de calor pelos preços, pela tarifa de demanda, pelos horários de saída, pelo conforto e pelo desgaste da bateria. Por baixo, uma camada de segurança em tempo real roda a cada segundo e só segue o plano até onde as regras permitem: o piso de consumo, os limites de injeção, o mínimo de 6 A por carro e os prazos de saída nunca dependem de o plano estar certo.</p></div>
      <div><h3>Do lado dos equipamentos</h3><p>Inversores e medidor são encontrados percorrendo a cadeia de modelos SunSpec. Os limites dos inversores são gravados sem prazo de reversão, então continuam valendo se o gateway cair; os carregadores recebem uma corrente de segurança de 6 A, que sozinha fica abaixo de 4,2 kW. Os comandos da distribuidora são gravados em disco, então uma redução sobrevive a um reinício.</p></div>`,
    planning: `<h2>O que o planejador MPC acrescenta, medido</h2>
    <p>Cada plano é um programa quadrático convexo sobre 96 quartos de hora: a dinâmica da bateria com perdas e um custo de desgaste por kWh ciclado, um modelo térmico de primeira ordem do prédio, a energia pedida por cada carro e seu horário de saída (como a ISO 15118 os passa ao carregador), a janela de redução anunciada pela distribuidora como restrição, os preços do dia seguinte e uma tarifa de demanda sobre o quarto de hora de maior compra. Ele é resolvido pelo Clarabel, um solver de pontos interiores escrito em Rust. É o mesmo no gateway, no estudo e nesta página. Os erros de previsão são tratados onde importam: a bateria só guarda reserva durante e pouco antes de uma redução esperada, o único momento em que a falta não pode ser comprada da rede.</p>
    <p style="margin-top:10px">Um estudo de Monte Carlo mediu isso. O depósito passou por 30 dias de clima aleatório na primavera e 30 no inverno, com os preços reais desses dias, e a distribuidora reduzindo das 17:30 às 19:30. O mesmo código rodou 720 vezes.</p>
    <div class="scroll" style="margin-top:12px"><table>
      <thead><tr><th>Por dia</th><th>Primavera · 6 abr 2025</th><th>Inverno · 20 jan 2025</th></tr></thead>
      <tbody>
        <tr><td>Só regras: energia, desgaste da bateria e tarifa de demanda</td><td class="mono">126,6 €</td><td class="mono">357,9 €</td></tr>
        <tr><td><b>MPC</b></td><td class="mono"><b>−25,6 ± 2,2 €</b> (−20%)</td><td class="mono"><b>−59,7 ± 2,4 €</b> (−17%)</td></tr>
        <tr><td>Maior quarto de hora de compra</td><td class="mono">89 → 34 kW</td><td class="mono">110 → 57 kW</td></tr>
        <tr><td>MPC que ignora a tarifa de demanda</td><td class="mono">−15,6 €, pico de até 93 kW</td><td class="mono">−44,4 €, pico de até 118 kW</td></tr>
        <tr><td>MPC probabilístico ou robusto</td><td class="mono">igual ao MPC</td><td class="mono">igual ao MPC</td></tr>
        <tr><td>Carros que saíram sem a energia pedida</td><td class="mono">0</td><td class="mono">0</td></tr>
      </tbody>
    </table></div>
    <p style="margin-top:10px">Quase todo o ganho vem dos preços, do pico e da inércia térmica do prédio. Os erros de previsão pesam bem menos, porque a camada de tempo real os absorve em segundos e o §14a garante um piso. Com a redução passada para a tarde, a reserva probabilística custou 5 € por dia na primavera e só comprou alguns segundos a menos acima do piso. Um plano que ignora a tarifa de demanda empilha a recarga nas horas mais baratas e aumenta o pico; para um local comercial alemão que paga <i>Leistungspreis</i>, isso pode dar prejuízo.</p>
    <p style="margin-top:10px"><a href="${GH}/blob/main/docs/mpc.md">A formulação, as escolhas de projeto e o estudo completo →</a></p>`,
    countries: `<h2>Regras de cada país para o mesmo controlador</h2>
    <div class="scroll"><table>
      <thead><tr><th></th><th>Consumo</th><th>Injeção</th><th>Fonte</th></tr></thead>
      <tbody>
        <tr><td><b>DE</b></td><td>§14a EnWG: a distribuidora pode reduzir bombas de calor, carregadores e baterias, mas precisa deixar Pmin,14a (4,2 kW por equipamento, com fator de simultaneidade para vários atrás de um EMS)</td><td>Valores de referência da distribuidora para instalações acima de 100 kW; sistemas novos sem medidor inteligente limitados a 60% (Solarspitzengesetz 2025)</td><td>BNetzA BK6-22-300, Anexo 1</td></tr>
        <tr><td><b>AT</b></td><td>Nas fontes consultadas não há mínimo legal como o do §14a; modelado como contrato de flexibilidade (potência mínima, minutos por dia)</td><td>Spitzenkappung: a distribuidora pode limitar a injeção de sistemas solares novos ou ampliados a até 70% da potência de pico; uma versão dinâmica está prevista para 2028</td><td>ElWG, BGBl. I 91/2025; ficha do BMWET</td></tr>
        <tr><td><b>CH</b></td><td>A flexibilidade é usada por contrato, com pagamento; o dono pode proibir usos que já existiam antes de 2026</td><td>Corte garantido e não pago de no máximo 3% da energia anual no ponto de conexão; ilimitado diante de ameaça imediata e grave</td><td>StromVG art. 17c; StromVV art. 19b–19d (em vigor desde 1 jan 2026)</td></tr>
      </tbody>
    </table></div>`,
    footer: `Local simulado com 120 kWp de solar no telhado, quatro carregadores de 22 kW, uma bomba de calor de 14 kW e uma bateria de 100 kWh. Os preços e o clima de 2025 são dados medidos; o depósito em si é modelado. Regras dos países conforme publicadas; não é aconselhamento jurídico. O gateway também roda como programa nativo contra equipamentos Modbus TCP reais (veja o README). Feito por Michael Hiarley Silva Andrade · <a href="https://github.com/maycu-byte">github.com/maycu-byte</a>`,
  },
  de: {
    intro: "Ein Standort-Controller in Rust zwischen Netzbetreiber und einem gewerblichen Prosumer-Standort. Er spricht IEC 60870-5-104 mit der Netzleitstelle und SunSpec Modbus mit Wechselrichtern, Ladepunkten, Wärmepumpe und Batterie. Ein modellprädiktiver Planer (MPC) plant den Standort nach echten Day-Ahead-Preisen; eine Echtzeit-Sicherheitsschicht setzt die Regeln des jeweiligen Landes um: §14a EnWG in Deutschland, die Spitzenkappung nach ElWG in Österreich, das 3-%-Abregelungsbudget und Flexibilitätsverträge in der Schweiz. Was unten läuft, ist dieser Code, kompiliert zu WebAssembly.",
    howCols: `<div><h3>Richtung Netzbetreiber</h3><p>Das Gateway ist eine IEC-60870-5-104-Unterstation, das Protokoll, das deutsche Netzbetreiber für die Fernsteuerung von Anlagen ab 100 kW verlangen. Befehle werden bestätigt (oder negativ quittiert), Messwerte gehen spontan mit Zeitstempel hinaus, und die Verbindung läuft über TLS mit Client-Zertifikaten im Sinne der IEC 62351-3. Der Protokollstack ist von Grund auf neu geschrieben und gegen lib60870 getestet.</p></div>
      <div><h3>Zwei Regelungsebenen</h3><p>Ein Planer (MPC) schaut alle 15 Minuten 24 Stunden voraus und plant Batterie, Fahrzeuge und Wärmepumpe nach Preisen, Leistungspreis, Abfahrtszeiten, Komfort und Batteriealterung. Darunter läuft sekündlich eine Echtzeit-Sicherheitsschicht, die dem Plan nur so weit folgt, wie die Regeln es erlauben: Mindestleistung, Einspeisegrenzen, das 6-A-Minimum je Fahrzeug und Abfahrtszeiten hängen nie davon ab, dass der Plan stimmt.</p></div>
      <div><h3>Richtung Geräte</h3><p>Wechselrichter und Zähler werden über die SunSpec-Modellkette gefunden. Wechselrichtergrenzen werden ohne Rückfall-Timeout geschrieben und bleiben daher aktiv, wenn das Gateway ausfällt; Ladepunkte erhalten einen Rückfallstrom von 6 A, der allein unter 4,2 kW liegt. Befehle des Netzbetreibers werden gespeichert, eine Dimmung übersteht also einen Neustart.</p></div>`,
    planning: `<h2>Was der MPC-Planer bringt, gemessen</h2>
    <p>Jeder Plan ist ein konvexes quadratisches Programm über 96 Viertelstunden: Batteriedynamik mit Verlusten und Alterungskosten je zyklierter kWh, ein thermisches Modell erster Ordnung des Gebäudes, der Energiebedarf und die Abfahrtszeit jedes Fahrzeugs (wie ISO 15118 sie an den Ladepunkt übergibt), das angekündigte Dimmfenster des Netzbetreibers als Nebenbedingung, Day-Ahead-Preise und ein Leistungspreis auf die höchste Bezugsviertelstunde. Gelöst wird es mit Clarabel, einem in Rust geschriebenen Innere-Punkte-Löser. Derselbe Löser läuft im Gateway, in der Studie und auf dieser Seite. Prognosefehler werden dort behandelt, wo sie zählen: die Batterie hält nur während und kurz vor einer erwarteten Dimmung eine Reserve, dem einzigen Zeitpunkt, zu dem ein Fehlbetrag nicht aus dem Netz gekauft werden kann.</p>
    <p style="margin-top:10px">Eine Monte-Carlo-Studie hat das gemessen. Das Depot lief an je 30 Tagen mit zufälligem Wetter im Frühling und im Winter, zu den echten Preisen dieser Tage, mit einer Dimmung von 17:30 bis 19:30. Derselbe Code lief 720-mal.</p>
    <div class="scroll" style="margin-top:12px"><table>
      <thead><tr><th>Pro Tag</th><th>Frühling · 6. Apr. 2025</th><th>Winter · 20. Jan. 2025</th></tr></thead>
      <tbody>
        <tr><td>Nur Regeln: Energie, Batteriealterung und Leistungspreis</td><td class="mono">126,6 €</td><td class="mono">357,9 €</td></tr>
        <tr><td><b>MPC</b></td><td class="mono"><b>−25,6 ± 2,2 €</b> (−20 %)</td><td class="mono"><b>−59,7 ± 2,4 €</b> (−17 %)</td></tr>
        <tr><td>Höchste Bezugsviertelstunde</td><td class="mono">89 → 34 kW</td><td class="mono">110 → 57 kW</td></tr>
        <tr><td>MPC ohne Leistungspreis</td><td class="mono">−15,6 €, Spitze bis 93 kW</td><td class="mono">−44,4 €, Spitze bis 118 kW</td></tr>
        <tr><td>Chance-constrained oder robuster MPC</td><td class="mono">wie MPC</td><td class="mono">wie MPC</td></tr>
        <tr><td>Fahrzeuge, die ohne ihre Energie abfuhren</td><td class="mono">0</td><td class="mono">0</td></tr>
      </tbody>
    </table></div>
    <p style="margin-top:10px">Der Nutzen kommt vor allem aus Preisen, der Lastspitze und der thermischen Masse des Gebäudes. Prognosefehler wiegen viel weniger, weil die Echtzeitschicht sie in Sekunden abfängt und §14a eine Mindestleistung garantiert. Mit der Dimmung am Nachmittag kostete die Chance-constrained-Reserve im Frühling 5 € am Tag und brachte nur wenige Sekunden weniger über der Mindestleistung. Ein Plan ohne Leistungspreis stapelt das Laden in die billigsten Stunden und erhöht die Spitze; für einen deutschen Gewerbestandort mit <i>Leistungspreis</i> kann das Geld kosten.</p>
    <p style="margin-top:10px"><a href="${GH}/blob/main/docs/mpc.md">Formulierung, Entwurfsentscheidungen und die vollständige Studie →</a></p>`,
    countries: `<h2>Länderregeln für denselben Controller</h2>
    <div class="scroll"><table>
      <thead><tr><th></th><th>Bezug</th><th>Einspeisung</th><th>Quelle</th></tr></thead>
      <tbody>
        <tr><td><b>DE</b></td><td>§14a EnWG: der Netzbetreiber darf Wärmepumpen, Ladepunkte und Batterien dimmen, muss aber Pmin,14a lassen (4,2 kW je Gerät, Gleichzeitigkeitsfaktor für mehrere hinter einem EMS)</td><td>Sollwerte des Netzbetreibers für Anlagen über 100 kW; Neuanlagen ohne Smart Meter auf 60 % begrenzt (Solarspitzengesetz 2025)</td><td>BNetzA BK6-22-300, Anlage 1</td></tr>
        <tr><td><b>AT</b></td><td>In den geprüften Quellen keine gesetzliche Mindestleistung wie nach §14a; modelliert als Flexibilitätsvertrag (Mindestleistung, Minuten pro Tag)</td><td>Spitzenkappung: der Netzbetreiber darf die Einspeisung neuer oder erweiterter PV auf bis zu 70 % der Modulspitzenleistung begrenzen; eine dynamische Variante ist für 2028 geplant</td><td>ElWG, BGBl. I 91/2025; Factsheet des BMWET</td></tr>
        <tr><td><b>CH</b></td><td>Flexibilität wird per Vertrag und gegen Vergütung genutzt; der Eigentümer kann Nutzungen untersagen, die vor 2026 bestanden</td><td>Garantierte, unvergütete Abregelung von höchstens 3 % der Jahresenergie am Anschlusspunkt; unbegrenzt bei unmittelbarer, erheblicher Gefährdung</td><td>StromVG Art. 17c; StromVV Art. 19b–19d (in Kraft seit 1. Jan. 2026)</td></tr>
      </tbody>
    </table></div>`,
    footer: `Simulierter Standort mit 120 kWp PV auf dem Dach, vier 22-kW-Ladepunkten, einer 14-kW-Wärmepumpe und einer 100-kWh-Batterie. Preise und Wetter 2025 sind Messdaten; das Depot selbst ist modelliert. Länderregeln wie veröffentlicht; keine Rechtsberatung. Das Gateway läuft auch als natives Programm gegen echte Modbus-TCP-Geräte (siehe README). Erstellt von Michael Hiarley Silva Andrade · <a href="https://github.com/maycu-byte">github.com/maycu-byte</a>`,
  },
};

// Short texts in the architecture drawing and the IEC 104 point list,
// swapped as text nodes like the rest of the interface.
export const EXTRA = {
  pt: {
    "How it works": "Como funciona", "SCADA · Netzleitstelle": "SCADA · centro de controle", "tested with c104 / lib60870": "testado com c104 / lib60870", "TLS, client certs": "TLS, certificados de cliente",
    "iec104: controlled station, sans-IO link layer": "iec104: estação controlada, enlace sem E/S", "planner: a 24 h convex QP (MPC) every 15 min": "planner: QP convexo de 24 h (MPC) a cada 15 min",
    "control: safety layer: DE/AT/CH rules, allocation": "control: camada de segurança: regras DE/AT/CH, divisão", "field: SunSpec discovery, watchdogs, reconnect": "field: descoberta SunSpec, watchdogs, reconexão",
    "api: JSON / WebSocket for this dashboard": "api: JSON / WebSocket para este painel", "Depot devices": "Equipamentos do depósito", "2 × 60 kW PV inverters": "2 inversores solares de 60 kW",
    "grid meter (SunSpec 203)": "medidor da rede (SunSpec 203)", "4 × 22 kW chargers": "4 carregadores de 22 kW", "heat pump · 100 kWh battery": "bomba de calor · bateria de 100 kWh",
    "IEC 104 point list, common address 1": "Lista de pontos IEC 104, endereço comum 1", "Type": "Tipo", "Direction": "Direção", "Meaning": "Significado",
    "DSO → site": "distribuidora → local", "site → DSO": "local → distribuidora",
    "Reduce consumption ON / OFF (§14a in DE, contract in AT/CH)": "Reduzir consumo LIGA / DESLIGA (§14a na DE, contrato na AT/CH)",
    "Feed-in limit, % of installed PV (0–100; other values are refused)": "Limite de injeção, % da potência solar instalada (0–100; outros valores são recusados)",
    "Emergency ON / OFF: overrides day limits, budgets and opt-outs": "Emergência LIGA / DESLIGA: passa por cima de limites diários, orçamentos e recusas",
    "Active power at the grid connection, kW (+ import)": "Potência ativa na ligação com a rede, kW (+ compra)", "PV active power, kW": "Potência solar, kW",
    "Controllable devices (steuVE), kW": "Equipamentos controláveis (steuVE), kW", "steuVE power drawn from the grid, kW (the quantity §14a limits)": "Potência dos steuVE tirada da rede, kW (a grandeza que o §14a limita)",
    "Consumption floor while dimmed, kW (Pmin,14a in DE)": "Piso de consumo durante a redução, kW (Pmin,14a na DE)", "Feed-in limit in force, % (feedback of 5002)": "Limite de injeção em vigor, % (retorno do 5002)",
    "PV limit sent to the inverters, %": "Limite solar enviado aos inversores, %", "Feed-in limit in force after country rules, %": "Limite de injeção em vigor depois das regras do país, %",
    "Battery power, kW (+ charging)": "Potência da bateria, kW (+ carregando)", "Battery state of charge, %": "Carga da bateria, %", "Consumption dimmed today, minutes": "Consumo reduzido hoje, minutos",
    "PV energy curtailed this year, kWh": "Energia solar cortada neste ano, kWh", "Free curtailment budget used, % (CH)": "Orçamento de corte gratuito usado, % (CH)",
    "Consumption dimming active (feedback of 5001)": "Redução de consumo ativa (retorno do 5001)", "Gradual release in progress": "Retorno gradual em andamento",
    "Meter fallback (grid measurement lost)": "Modo de segurança do medidor (medição da rede perdida)", "Field device fault": "Falha em equipamento",
    "Emergency active (feedback of 5003)": "Emergência ativa (retorno do 5003)", "Contract day limit reached, dimming refused": "Limite diário do contrato atingido, redução recusada",
    "Curtailment budget used up": "Orçamento de corte esgotado",
  },
  de: {
    "How it works": "So funktioniert es", "SCADA · Netzleitstelle": "SCADA · Netzleitstelle", "tested with c104 / lib60870": "getestet mit c104 / lib60870", "TLS, client certs": "TLS, Client-Zertifikate",
    "iec104: controlled station, sans-IO link layer": "iec104: Unterstation, Sans-IO-Verbindungsschicht", "planner: a 24 h convex QP (MPC) every 15 min": "planner: konvexes 24-h-QP (MPC) alle 15 min",
    "control: safety layer: DE/AT/CH rules, allocation": "control: Sicherheitsschicht: DE/AT/CH-Regeln, Aufteilung", "field: SunSpec discovery, watchdogs, reconnect": "field: SunSpec-Erkennung, Watchdogs, Wiederverbindung",
    "api: JSON / WebSocket for this dashboard": "api: JSON / WebSocket für dieses Dashboard", "Depot devices": "Geräte im Depot", "2 × 60 kW PV inverters": "2 × 60-kW-PV-Wechselrichter",
    "grid meter (SunSpec 203)": "Netzzähler (SunSpec 203)", "4 × 22 kW chargers": "4 × 22-kW-Ladepunkte", "heat pump · 100 kWh battery": "Wärmepumpe · 100-kWh-Batterie",
    "IEC 104 point list, common address 1": "IEC-104-Datenpunktliste, gemeinsame Adresse 1", "Type": "Typ", "Direction": "Richtung", "Meaning": "Bedeutung",
    "DSO → site": "Netzbetreiber → Standort", "site → DSO": "Standort → Netzbetreiber",
    "Reduce consumption ON / OFF (§14a in DE, contract in AT/CH)": "Bezug reduzieren EIN / AUS (§14a in DE, Vertrag in AT/CH)",
    "Feed-in limit, % of installed PV (0–100; other values are refused)": "Einspeisegrenze, % der installierten PV (0–100; andere Werte werden abgelehnt)",
    "Emergency ON / OFF: overrides day limits, budgets and opt-outs": "Notfall EIN / AUS: setzt Tageslimits, Budgets und Widersprüche außer Kraft",
    "Active power at the grid connection, kW (+ import)": "Wirkleistung am Netzanschluss, kW (+ Bezug)", "PV active power, kW": "PV-Wirkleistung, kW",
    "Controllable devices (steuVE), kW": "Steuerbare Verbrauchseinrichtungen (steuVE), kW", "steuVE power drawn from the grid, kW (the quantity §14a limits)": "Netzbezug der steuVE, kW (die nach §14a begrenzte Größe)",
    "Consumption floor while dimmed, kW (Pmin,14a in DE)": "Mindestleistung während der Dimmung, kW (Pmin,14a in DE)", "Feed-in limit in force, % (feedback of 5002)": "Aktive Einspeisegrenze, % (Rückmeldung zu 5002)",
    "PV limit sent to the inverters, %": "An die Wechselrichter gesendete PV-Grenze, %", "Feed-in limit in force after country rules, %": "Aktive Einspeisegrenze nach Länderregeln, %",
    "Battery power, kW (+ charging)": "Batterieleistung, kW (+ Laden)", "Battery state of charge, %": "Ladezustand der Batterie, %", "Consumption dimmed today, minutes": "Heute gedimmter Bezug, Minuten",
    "PV energy curtailed this year, kWh": "In diesem Jahr abgeregelte PV-Energie, kWh", "Free curtailment budget used, % (CH)": "Genutztes kostenloses Abregelungsbudget, % (CH)",
    "Consumption dimming active (feedback of 5001)": "Bezugsdimmung aktiv (Rückmeldung zu 5001)", "Gradual release in progress": "Schrittweise Freigabe läuft",
    "Meter fallback (grid measurement lost)": "Zähler-Rückfallebene (Netzmessung verloren)", "Field device fault": "Störung eines Feldgeräts",
    "Emergency active (feedback of 5003)": "Notfall aktiv (Rückmeldung zu 5003)", "Contract day limit reached, dimming refused": "Vertragliches Tageslimit erreicht, Dimmung abgelehnt",
    "Curtailment budget used up": "Abregelungsbudget aufgebraucht",
    "What is simulated?": "Was wird simuliert?", "Live demo": "Live-Demo", "Data sources": "Datenquellen",
  },
};
EXTRA.pt["What is simulated?"] = "O que é simulado?";
EXTRA.pt["Live demo"] = "Demonstração ao vivo";
EXTRA.pt["Data sources"] = "Fontes dos dados";
EXTRA.pt["2025, day by day"] = "2025, dia a dia";
EXTRA.de["2025, day by day"] = "2025, Tag für Tag";

// Context, the calculator's reading guide and the data sources.
const SW = (c) => `<span class="sw" style="background:var(${c})"></span>`;
Object.assign(BLOCKS.pt, {
  context: `<h2>O que está sendo simulado</h2>
    <div class="ctx-grid">
      <div><span class="n">1</span><h3>Um depósito de entregas na Alemanha</h3><p>Um local comercial ligado à rede: 120 kWp de painéis solares no telhado, quatro carregadores de 22 kW para as <b>vans elétricas de entrega</b> (elas voltam no fim da tarde, carregam durante a noite e saem de manhã), uma bomba de calor de 14 kW que aquece o prédio e uma bateria de 100 kWh.</p></div>
      <div><span class="n">2</span><h3>Uma regra que deixa a distribuidora frear o consumo</h3><p>Nas noites de inverno todo mundo liga tudo ao mesmo tempo e o transformador do bairro pode sobrecarregar. O §14a EnWG alemão permite que a distribuidora <b>reduza</b> carregadores, bombas de calor e baterias por um tempo, desde que deixe um mínimo garantido.</p></div>
      <div><span class="n">3</span><h3>O gateway no meio</h3><p>O Grid Edge Gateway é o software entre a distribuidora e o depósito. Ele recebe os comandos da distribuidora (IEC 104), comanda os equipamentos (Modbus), respeita as regras e, se você escolher, planeja o dia pelo preço da energia. <b>Na demonstração ao vivo, você é a distribuidora:</b> clique em "Reduzir" e veja o local obedecer.</p></div>
      <div><span class="n">4</span><h3>O que acontece quando a redução acaba?</h3><p>Durante a redução, as vans ficam devendo carga. Quando ela termina, <b>todos os depósitos do bairro</b> recuperam ao mesmo tempo, e o pico pode ficar maior do que sem redução nenhuma. Isso é o <i>efeito de retorno</i>. A <a href="#calculator">calculadora do alimentador</a> simula muitos depósitos juntos para achar a forma menos prejudicial de religá-los.</p></div>
    </div>`,
  calcGuide: `<div><b>1. Descreva o bairro</b>Quantos depósitos dividem o transformador, quanto ele aguenta por depósito, quando as vans chegam, qual dia e quanto tempo dura a redução.</div>
      <div><b>2. Escolha como os depósitos voltam</b>Todos de uma vez, devagar (rampa), depois de uma espera aleatória ou em grupos. Cada depósito também pode planejar pelo preço. Clique em <i>Calcular</i>, ou em <i>Comparar todas as opções</i> para testar todas.</div>
      <div><b>3. Leia a noite</b><ul><li>${SW("--s-grid")}azul: a carga do bairro com a sua opção</li><li>${SW("--ink2")}tracejado: a mesma noite sem redução</li><li>${SW("--critical")}vermelho: o limite do transformador (azul acima dele é sobrecarga)</li><li>faixa roxa: a redução</li></ul></div>`,
  data: `<h2>Fontes dos dados: o que é medido e o que é modelado</h2>
    <div class="scroll"><table>
      <thead><tr><th>Dado</th><th>O que é usado</th><th>Fonte</th><th>Tipo</th></tr></thead>
      <tbody>
        <tr><td>Preços da energia</td><td>Preços horários do mercado do dia seguinte da zona Alemanha–Luxemburgo para todas as horas de 2025 (a partir de 1º de outubro, a média dos quatro produtos de 15 minutos); os dois dias em destaque são 20 de janeiro (pico de 583 €/MWh às 17:00) e 6 de abril (−115 €/MWh às 14:00)</td><td>SMARD.de (Bundesnetzagentur), zona DE-LU, CC BY 4.0, pela API do Energy-Charts · <code>devices/src/year2025_data.rs</code></td><td class="tag-real">real · Alemanha</td></tr>
        <tr><td>Regras de redução</td><td>Potência mínima Pmin,14a = 4,2 kW por equipamento, fator de simultaneidade, retorno gradual em 5 minutos, comprovação de cumprimento</td><td>Decisão BNetzA BK6-22-300, Anexo 1 (§14a EnWG)</td><td class="tag-real">real · Alemanha</td></tr>
        <tr><td>Regras de injeção</td><td>Limite de 60% para sistemas novos sem medidor inteligente; sem pagamento em horas de preço negativo</td><td>Solarspitzengesetz 2025; EEG §51</td><td class="tag-real">real · Alemanha</td></tr>
        <tr><td>Áustria e Suíça</td><td>Limite de injeção de 70%; orçamento de corte gratuito de 3%; contratos de flexibilidade</td><td>ElWG (BGBl. I 91/2025); StromVG art. 17c, StromVV art. 19b–19d</td><td class="tag-real">real · AT / CH</td></tr>
        <tr><td>Sol e temperatura, qualquer dia de 2025</td><td>Temperatura do ar e irradiação global horárias medidas em Stuttgart; produção solar = irradiação × 0,85</td><td>Arquivo histórico do Open-Meteo (reanálise ERA5, Copernicus/ECMWF), CC BY 4.0 · <code>devices/src/year2025_data.rs</code></td><td class="tag-real">real · Alemanha</td></tr>
        <tr><td>Sol e temperatura, os dois dias do estudo</td><td>Curva solar de céu limpo e temperatura diária do sudoeste da Alemanha, com nebulosidade sorteada por dia (ensolarado, misto, nublado), usada nos estudos de Monte Carlo e de retorno</td><td>Modelo em <code>devices/src/climate.rs</code>, parâmetros definidos à mão</td><td class="tag-syn">modelado</td></tr>
        <tr><td>Vans</td><td>Horário de chegada, energia pedida e horas conectada de cada van; "horários variados" desloca cada depósito em até ±1 h e ±30% de energia</td><td>Modelo em <code>devices/src/sim.rs</code> e <code>closedloop/src/feeder.rs</code></td><td class="tag-syn">modelado</td></tr>
        <tr><td>Prédio e outras cargas</td><td>Modelo térmico de primeira ordem do prédio; 10–40 kW de outros consumos</td><td>Modelo em <code>devices/src/sim.rs</code></td><td class="tag-syn">modelado</td></tr>
        <tr><td>Tarifa</td><td>Preço do dia seguinte + 12 ct/kWh de tarifas de rede e encargos; tarifa de demanda de 100 €/kW por ano</td><td>Valores ilustrativos, da ordem das tarifas comerciais alemãs</td><td class="tag-syn">suposto</td></tr>
      </tbody>
    </table></div>
    <div class="data-note">
      <h3>Dois dias em destaque, e todos os dias de 2025</h3>
      <p>Os botões trazem os dias mais difíceis do ano. <b>20 de janeiro de 2025</b> foi uma <i>Dunkelflaute</i>, com pouco vento, pouco sol e preço de 583 €/MWh à noite. Em dias assim, as distribuidoras alemãs mais tendem a reduzir bombas de calor e carregadores. <b>6 de abril de 2025</b> teve preços muito negativos ao meio-dia, a situação para a qual existem os limites de injeção.</p>
      <p style="margin-top:8px">Qualquer outro dia de 2025 também pode ser escolhido: ele roda com os preços reais e o clima medido daquele dia. A seção <a href="#year">2025, noite a noite</a> roda as 365 noites para um alimentador de 20 depósitos. Isso reproduz 2025 como ele aconteceu. Os preços do dia seguinte saem com um dia de antecedência, então não dá para prever um ano inteiro deles, e o planejador só precisa das próximas 24 horas.</p>
    </div>`,
});
Object.assign(BLOCKS.de, {
  context: `<h2>Was hier simuliert wird</h2>
    <div class="ctx-grid">
      <div><span class="n">1</span><h3>Ein Lieferdepot in Deutschland</h3><p>Ein gewerblicher Standort am Netz: 120 kWp Solarmodule auf dem Dach, vier 22-kW-Ladepunkte für die <b>elektrischen Lieferwagen</b> (sie kommen am späten Nachmittag zurück, laden über Nacht und fahren morgens los), eine 14-kW-Wärmepumpe, die das Gebäude heizt, und eine 100-kWh-Batterie.</p></div>
      <div><span class="n">2</span><h3>Eine Regel, mit der der Netzbetreiber bremsen darf</h3><p>An Winterabenden beziehen alle gleichzeitig Strom, und der Trafo im Viertel kann überlastet werden. Nach §14a EnWG darf der Netzbetreiber Ladepunkte, Wärmepumpen und Batterien eine Zeit lang <b>dimmen</b>, solange eine garantierte Mindestleistung bleibt.</p></div>
      <div><span class="n">3</span><h3>Das Gateway dazwischen</h3><p>Das Grid Edge Gateway ist die Software zwischen Netzbetreiber und Depot. Es empfängt die Befehle des Netzbetreibers (IEC 104), steuert die Geräte (Modbus), hält die Regeln ein und plant auf Wunsch den Tag nach dem Strompreis. <b>In der Live-Demo sind Sie der Netzbetreiber:</b> klicken Sie auf „Dimmen“ und sehen Sie zu, wie der Standort gehorcht.</p></div>
      <div><span class="n">4</span><h3>Was passiert, wenn die Reduzierung endet?</h3><p>Während der Reduzierung geraten die Lieferwagen in Rückstand. Endet sie, holen <b>alle Depots im Viertel</b> gleichzeitig auf, und die Spitze kann höher werden als ganz ohne Reduzierung. Das ist der <i>Nachholeffekt</i>. Der <a href="#calculator">Strang-Rechner</a> simuliert viele Depots zusammen, um den schonendsten Weg zurück zu finden.</p></div>
    </div>`,
  calcGuide: `<div><b>1. Das Viertel beschreiben</b>Wie viele Depots sich den Trafo teilen, wie viel er je Depot trägt, wann die Lieferwagen ankommen, welcher Tag und wie lange die Reduzierung dauert.</div>
      <div><b>2. Wählen, wie die Depots zurückkehren</b>Alle sofort, langsam (Rampe), nach einer zufälligen Wartezeit oder in Gruppen. Jedes Depot kann auch nach Preis planen. Klicken Sie auf <i>Berechnen</i> oder auf <i>Alle Optionen vergleichen</i>.</div>
      <div><b>3. Den Abend lesen</b><ul><li>${SW("--s-grid")}blau: die Last des Viertels mit Ihrer Option</li><li>${SW("--ink2")}gestrichelt: derselbe Abend ohne Reduzierung</li><li>${SW("--critical")}rot: die Grenze des Trafos (blau darüber ist Überlast)</li><li>lila Band: die Reduzierung</li></ul></div>`,
  data: `<h2>Datenquellen: was gemessen und was modelliert ist</h2>
    <div class="scroll"><table>
      <thead><tr><th>Daten</th><th>Was verwendet wird</th><th>Quelle</th><th>Art</th></tr></thead>
      <tbody>
        <tr><td>Strompreise</td><td>Stündliche Day-Ahead-Preise der Gebotszone Deutschland–Luxemburg für jede Stunde 2025 (ab 1. Oktober der Mittelwert der vier 15-Minuten-Produkte); hervorgehoben sind der 20. Januar (Spitze 583 €/MWh um 17:00) und der 6. April (−115 €/MWh um 14:00)</td><td>SMARD.de (Bundesnetzagentur), Gebotszone DE-LU, CC BY 4.0, über die Energy-Charts-API · <code>devices/src/year2025_data.rs</code></td><td class="tag-real">echt · Deutschland</td></tr>
        <tr><td>Regeln für das Dimmen</td><td>Mindestleistung Pmin,14a = 4,2 kW je Gerät, Gleichzeitigkeitsfaktor, schrittweise Freigabe über 5 Minuten, Nachweis</td><td>BNetzA-Festlegung BK6-22-300, Anlage 1 (§14a EnWG)</td><td class="tag-real">echt · Deutschland</td></tr>
        <tr><td>Einspeiseregeln</td><td>60-%-Grenze für Neuanlagen ohne Smart Meter; keine Vergütung in Stunden mit negativen Preisen</td><td>Solarspitzengesetz 2025; EEG §51</td><td class="tag-real">echt · Deutschland</td></tr>
        <tr><td>Österreich und Schweiz</td><td>70-%-Einspeisegrenze; 3 % kostenloses Abregelungsbudget; Flexibilitätsverträge</td><td>ElWG (BGBl. I 91/2025); StromVG Art. 17c, StromVV Art. 19b–19d</td><td class="tag-real">echt · AT / CH</td></tr>
        <tr><td>Sonne und Temperatur, jeder Tag 2025</td><td>Stündlich gemessene Lufttemperatur und Globalstrahlung in Stuttgart; PV-Leistung = Strahlung × 0,85</td><td>Historisches Archiv von Open-Meteo (ERA5-Reanalyse, Copernicus/ECMWF), CC BY 4.0 · <code>devices/src/year2025_data.rs</code></td><td class="tag-real">echt · Deutschland</td></tr>
        <tr><td>Sonne und Temperatur, die zwei Studientage</td><td>Solarkurve bei klarem Himmel und Tagestemperatur für Südwestdeutschland, mit zufälliger Bewölkung je Tag (sonnig, gemischt, bedeckt), für die Monte-Carlo- und die Nachholeffekt-Studie</td><td>Modell in <code>devices/src/climate.rs</code>, Parameter von Hand gesetzt</td><td class="tag-syn">modelliert</td></tr>
        <tr><td>Lieferwagen</td><td>Ankunftszeit, gewünschte Energie und Standzeit je Fahrzeug; „unterschiedlich“ verschiebt jedes Depot um bis zu ±1 h und ±30 % Energie</td><td>Modell in <code>devices/src/sim.rs</code> und <code>closedloop/src/feeder.rs</code></td><td class="tag-syn">modelliert</td></tr>
        <tr><td>Gebäude und übrige Lasten</td><td>Thermisches Modell erster Ordnung; 10–40 kW sonstiger Verbrauch</td><td>Modell in <code>devices/src/sim.rs</code></td><td class="tag-syn">modelliert</td></tr>
        <tr><td>Tarif</td><td>Day-Ahead-Preis + 12 ct/kWh Netzentgelte und Umlagen; Leistungspreis 100 €/kW im Jahr</td><td>Beispielwerte in der Größenordnung deutscher Gewerbetarife</td><td class="tag-syn">angenommen</td></tr>
      </tbody>
    </table></div>
    <div class="data-note">
      <h3>Zwei hervorgehobene Tage, und jeder Tag 2025</h3>
      <p>Die Schaltflächen zeigen die schwierigsten Tage des Jahres. Der <b>20. Januar 2025</b> war eine <i>Dunkelflaute</i> mit wenig Wind, wenig Sonne und 583 €/MWh am Abend. An solchen Tagen dimmen deutsche Netzbetreiber am ehesten Wärmepumpen und Ladepunkte. Am <b>6. April 2025</b> waren die Preise mittags stark negativ, der Fall, für den es Einspeisegrenzen gibt.</p>
      <p style="margin-top:8px">Jeder andere Tag 2025 lässt sich ebenfalls wählen: er läuft mit den echten Preisen und dem gemessenen Wetter dieses Tages. Der Abschnitt <a href="#year">2025, Abend für Abend</a> rechnet alle 365 Abende für einen Strang mit 20 Depots. Das spielt 2025 so nach, wie es war. Day-Ahead-Preise erscheinen einen Tag im Voraus, ein ganzes Jahr lässt sich also nicht vorhersagen, und der Planer braucht nur die nächsten 24 Stunden.</p>
    </div>`,
});
