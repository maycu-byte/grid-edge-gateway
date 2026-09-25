// eebus-bridge: the EEBUS side of grid-edge-gateway.
//
// It acts as the Controllable System of the LPC use case (Limitation of
// Power Consumption), the way an energy manager behind an FNN control box
// receives §14a limits in Germany. The control box, as energy guard, writes
// a consumption limit; the bridge approves it, keeps the heartbeat and the
// failsafe rules, and passes the limit in force to the gateway as one JSON
// line over a local TCP socket.
//
// The EEBUS stack is eebus-go (github.com/enbility/eebus-go, MIT).
//
// Usage:
//
//	eebus-bridge -remote-ski <control box SKI> [-port 4713] [-gateway 127.0.0.1:4712]
//	             [-cert eebus.crt -key eebus.key] [-nominal-max-w 32000]
//	             [-failsafe-w 4200] [-failsafe-min 2h]
//
// On the first run the certificate is created and saved; its SKI, printed
// at start, is what the control box has to be paired with.
package main

import (
	"crypto/ecdsa"
	"crypto/tls"
	"crypto/x509"
	"encoding/pem"
	"errors"
	"flag"
	"fmt"
	"log"
	"os"
	"os/signal"
	"sync"
	"syscall"
	"time"

	"github.com/enbility/eebus-go/api"
	"github.com/enbility/eebus-go/service"
	ucapi "github.com/enbility/eebus-go/usecases/api"
	cslpc "github.com/enbility/eebus-go/usecases/cs/lpc"
	shipapi "github.com/enbility/ship-go/api"
	"github.com/enbility/ship-go/cert"
	spineapi "github.com/enbility/spine-go/api"
	"github.com/enbility/spine-go/model"
)

type bridge struct {
	remoteSKI string
	svc       *service.Service
	lpc       ucapi.CsLPCInterface

	mu    sync.Mutex
	state *State
	link  *Link
}

func main() {
	port := flag.Int("port", 4713, "EEBUS (SHIP) port")
	remoteSKI := flag.String("remote-ski", "", "SKI of the control box (energy guard) to pair with")
	gateway := flag.String("gateway", "127.0.0.1:4712", "the gateway's [eebus] bind address")
	certPath := flag.String("cert", "eebus.crt", "certificate file (created if missing)")
	keyPath := flag.String("key", "eebus.key", "private key file (created if missing)")
	nominalMaxW := flag.Float64("nominal-max-w", 32000, "most the site's controllable devices can draw, W")
	failsafeW := flag.Float64("failsafe-w", 4200, "failsafe consumption limit until the guard sets one, W")
	failsafeMin := flag.Duration("failsafe-min", 2*time.Hour, "failsafe minimum duration until the guard sets one")
	flag.Parse()

	certificate, err := loadOrCreateCert(*certPath, *keyPath)
	if err != nil {
		log.Fatal(err)
	}
	ski, err := cert.SkiFromCertificate(certificate.Leaf)
	if err != nil {
		log.Fatal(err)
	}
	log.Printf("local SKI: %s", ski)

	b := &bridge{
		remoteSKI: *remoteSKI,
		state:     NewState(time.Now(), *failsafeW, *failsafeMin),
		link:      NewLink(*gateway),
	}
	if err := b.start(*port, certificate, *nominalMaxW); err != nil {
		log.Fatal(err)
	}

	sig := make(chan os.Signal, 1)
	signal.Notify(sig, os.Interrupt, syscall.SIGTERM)
	stop := make(chan struct{})
	go func() { <-sig; close(stop) }()
	b.loop(stop)
}

// loop checks the heartbeat and updates the gateway every second.
func (b *bridge) loop(stop <-chan struct{}) {
	tick := time.NewTicker(time.Second)
	defer tick.Stop()
	for {
		select {
		case <-stop:
			b.svc.Shutdown()
			return
		case now := <-tick.C:
			b.mu.Lock()
			b.state.Heartbeat(b.lpc.IsHeartbeatWithinDuration(), now)
			b.link.Send(b.state.Output(now), now)
			b.mu.Unlock()
		}
	}
}

func (b *bridge) start(port int, certificate tls.Certificate, nominalMaxW float64) error {
	cfg, err := api.NewConfiguration(
		"grid-edge-gateway", "grid-edge-gateway", "eebus-bridge", "0001",
		model.DeviceTypeTypeEnergyManagementSystem,
		[]model.EntityTypeType{model.EntityTypeTypeCEM},
		port, certificate, 4*time.Second)
	if err != nil {
		return err
	}
	cfg.SetAlternateIdentifier("grid-edge-gateway-eebus-bridge")
	b.svc = service.NewService(cfg, b)
	b.svc.SetLogging(b)
	if err := b.svc.Setup(); err != nil {
		return err
	}
	entity := b.svc.LocalDevice().EntityForType(model.EntityTypeTypeCEM)
	b.lpc = cslpc.NewLPC(entity, b.onLPC)
	b.svc.AddUseCase(b.lpc)

	st := b.state
	if err := errors.Join(
		b.lpc.SetConsumptionNominalMax(nominalMaxW),
		b.lpc.SetConsumptionLimit(ucapi.LoadLimit{Value: nominalMaxW, IsChangeable: true, IsActive: false}),
		b.lpc.SetFailsafeConsumptionActivePowerLimit(st.failsafeW, true),
		b.lpc.SetFailsafeDurationMinimum(st.failsafeMin, true),
	); err != nil {
		return err
	}
	if b.remoteSKI == "" {
		log.Print("no -remote-ski: waiting to be paired")
	} else {
		b.svc.RegisterRemoteSKI(b.remoteSKI)
	}
	b.svc.Start()
	return nil
}

// onLPC handles the energy guard's writes.
func (b *bridge) onLPC(ski string, _ spineapi.DeviceRemoteInterface, _ spineapi.EntityRemoteInterface, event api.EventType) {
	now := time.Now()
	switch event {
	case cslpc.WriteApprovalRequired:
		// Under §14a the site has to follow the grid operator's limit; the
		// gateway itself never lets it fall below Pmin,14a.
		for counter, l := range b.lpc.PendingConsumptionLimits() {
			ok := l.Value >= 0
			reason := ""
			if !ok {
				reason = "negative consumption limit"
			}
			log.Printf("limit write %d: %.0f W, active %v, %v: approved %v", counter, l.Value, l.IsActive, l.Duration, ok)
			b.lpc.ApproveOrDenyConsumptionLimit(counter, ok, reason)
		}
	case cslpc.DataUpdateLimit:
		l, err := b.lpc.ConsumptionLimit()
		if err != nil {
			log.Printf("limit: %v", err)
			return
		}
		b.mu.Lock()
		b.state.SetLimit(l.IsActive, l.Value, l.Duration, now)
		b.mu.Unlock()
	case cslpc.DataUpdateFailsafeConsumptionActivePowerLimit, cslpc.DataUpdateFailsafeDurationMinimum:
		w, _, errW := b.lpc.FailsafeConsumptionActivePowerLimit()
		d, _, errD := b.lpc.FailsafeDurationMinimum()
		if errW != nil || errD != nil {
			log.Printf("failsafe values: %v %v", errW, errD)
			return
		}
		b.mu.Lock()
		b.state.SetFailsafe(w, d)
		b.mu.Unlock()
		log.Printf("failsafe: %.0f W for at least %v", w, d)
	}
}

// loadOrCreateCert keeps the certificate across restarts: the SKI derived
// from it is what the control box is paired with.
func loadOrCreateCert(certPath, keyPath string) (tls.Certificate, error) {
	if c, err := tls.LoadX509KeyPair(certPath, keyPath); err == nil {
		c.Leaf, err = x509.ParseCertificate(c.Certificate[0])
		return c, err
	}
	c, err := cert.CreateCertificate("grid-edge-gateway", "grid-edge-gateway", "DE", "eebus-bridge")
	if err != nil {
		return c, err
	}
	key, err := x509.MarshalECPrivateKey(c.PrivateKey.(*ecdsa.PrivateKey))
	if err != nil {
		return c, err
	}
	if err := errors.Join(
		os.WriteFile(certPath, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: c.Certificate[0]}), 0o644),
		os.WriteFile(keyPath, pem.EncodeToMemory(&pem.Block{Type: "EC PRIVATE KEY", Bytes: key}), 0o600),
	); err != nil {
		return c, err
	}
	c.Leaf, err = x509.ParseCertificate(c.Certificate[0])
	return c, err
}

// api.ServiceReaderInterface

func (b *bridge) RemoteSKIConnected(_ api.ServiceInterface, ski string) {
	log.Printf("control box %s connected", ski)
}

func (b *bridge) RemoteSKIDisconnected(_ api.ServiceInterface, ski string) {
	log.Printf("control box %s disconnected", ski)
}

func (b *bridge) VisibleRemoteServicesUpdated(api.ServiceInterface, []shipapi.RemoteService) {}

func (b *bridge) ServiceShipIDUpdate(string, string) {}

func (b *bridge) ServicePairingDetailUpdate(ski string, detail *shipapi.ConnectionStateDetail) {
	if detail.State() == shipapi.ConnectionStateRemoteDeniedTrust {
		log.Printf("control box %s denied trust", ski)
	}
}

func (b *bridge) AllowWaitingForTrust(ski string) bool {
	return b.remoteSKI == "" || ski == b.remoteSKI
}

// logging.LoggingInterface: eebus-go's own messages, debug and trace left out.

func (b *bridge) Trace(...interface{})          {}
func (b *bridge) Tracef(string, ...interface{}) {}
func (b *bridge) Debug(...interface{})          {}
func (b *bridge) Debugf(string, ...interface{}) {}
func (b *bridge) Info(args ...interface{})      { log.Print(append([]interface{}{"eebus: "}, args...)...) }
func (b *bridge) Infof(f string, args ...interface{}) {
	log.Print("eebus: " + fmt.Sprintf(f, args...))
}
func (b *bridge) Error(args ...interface{}) {
	log.Print(append([]interface{}{"eebus error: "}, args...)...)
}
func (b *bridge) Errorf(f string, args ...interface{}) {
	log.Print("eebus error: " + fmt.Sprintf(f, args...))
}
