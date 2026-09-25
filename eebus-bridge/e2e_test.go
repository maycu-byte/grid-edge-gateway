//go:build e2e

// End-to-end: an energy guard built on eebus-go's own LPC client (as in its
// control box example) pairs with the bridge over SHIP on this machine,
// writes a consumption limit, and the limit reaches a stand-in gateway
// socket as a JSON line. Needs mDNS on the local network interface:
//
//	go test -tags e2e -run TestEnergyGuard -v
package main

import (
	"bufio"
	"encoding/json"
	"net"
	"path/filepath"
	"testing"
	"time"

	"github.com/enbility/eebus-go/api"
	"github.com/enbility/eebus-go/service"
	ucapi "github.com/enbility/eebus-go/usecases/api"
	eglpc "github.com/enbility/eebus-go/usecases/eg/lpc"
	shipapi "github.com/enbility/ship-go/api"
	"github.com/enbility/ship-go/cert"
	spineapi "github.com/enbility/spine-go/api"
	"github.com/enbility/spine-go/model"
)

// guard is the control box side: it writes a limit as soon as the bridge
// announces LPC support.
type guard struct {
	svc       *service.Service
	lpc       ucapi.EgLPCInterface
	remoteSKI string
	limitW    float64
	written   chan error
}

func (g *guard) onLPC(_ string, _ spineapi.DeviceRemoteInterface, entity spineapi.EntityRemoteInterface, event api.EventType) {
	if event != eglpc.UseCaseSupportUpdate {
		return
	}
	// The limit descriptions arrive after the use case support: retry the
	// write until they are there.
	go func() {
		limit := ucapi.LoadLimit{Duration: 10 * time.Minute, IsActive: true, Value: g.limitW}
		var err error
		for range 30 {
			_, err = g.lpc.WriteConsumptionLimit(entity, limit, func(msg model.ResultDataType) {
				if msg.ErrorNumber != nil && *msg.ErrorNumber != model.ErrorNumberTypeNoError {
					g.written <- errFromResult(msg)
					return
				}
				g.written <- nil
			})
			if err == nil {
				return
			}
			time.Sleep(500 * time.Millisecond)
		}
		g.written <- err
	}()
}

type resultErr string

func (e resultErr) Error() string { return string(e) }

func errFromResult(msg model.ResultDataType) error {
	if msg.Description != nil {
		return resultErr(string(*msg.Description))
	}
	return resultErr("limit rejected")
}

func (g *guard) RemoteSKIConnected(api.ServiceInterface, string)                            {}
func (g *guard) RemoteSKIDisconnected(api.ServiceInterface, string)                         {}
func (g *guard) VisibleRemoteServicesUpdated(api.ServiceInterface, []shipapi.RemoteService) {}
func (g *guard) ServiceShipIDUpdate(string, string)                                         {}
func (g *guard) ServicePairingDetailUpdate(string, *shipapi.ConnectionStateDetail)          {}
func (g *guard) AllowWaitingForTrust(ski string) bool                                       { return ski == g.remoteSKI }

func newCert(t *testing.T, name string) (string, string) {
	t.Helper()
	dir := t.TempDir()
	c, k := filepath.Join(dir, name+".crt"), filepath.Join(dir, name+".key")
	if _, err := loadOrCreateCert(c, k); err != nil {
		t.Fatal(err)
	}
	return c, k
}

func TestEnergyGuardLimitReachesTheGateway(t *testing.T) {
	// Stand-in for the gateway's [eebus] socket.
	gw, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer gw.Close()
	lines := make(chan Output, 64)
	go func() {
		conn, err := gw.Accept()
		if err != nil {
			return
		}
		sc := bufio.NewScanner(conn)
		for sc.Scan() {
			var o Output
			if json.Unmarshal(sc.Bytes(), &o) == nil {
				lines <- o
			}
		}
	}()

	bc, bk := newCert(t, "bridge")
	bcert, _ := loadOrCreateCert(bc, bk)
	bSKI, _ := cert.SkiFromCertificate(bcert.Leaf)
	gc, gk := newCert(t, "guard")
	gcert, _ := loadOrCreateCert(gc, gk)
	gSKI, _ := cert.SkiFromCertificate(gcert.Leaf)

	b := &bridge{remoteSKI: gSKI, state: NewState(time.Now(), 4200, 2*time.Hour), link: NewLink(gw.Addr().String())}
	if err := b.start(47130, bcert, 32000); err != nil {
		t.Fatal(err)
	}
	stop := make(chan struct{})
	go b.loop(stop)
	defer close(stop)

	g := &guard{remoteSKI: bSKI, limitW: 7000, written: make(chan error, 4)}
	cfg, err := api.NewConfiguration("Test", "Test", "ControlBox", "0002",
		model.DeviceTypeTypeElectricitySupplySystem, []model.EntityTypeType{model.EntityTypeTypeGridGuard},
		47131, gcert, 60*time.Second)
	if err != nil {
		t.Fatal(err)
	}
	g.svc = service.NewService(cfg, g)
	if err := g.svc.Setup(); err != nil {
		t.Fatal(err)
	}
	g.lpc = eglpc.NewLPC(g.svc.LocalDevice().EntityForType(model.EntityTypeTypeGridGuard), g.onLPC)
	g.svc.AddUseCase(g.lpc)
	g.svc.RegisterRemoteSKI(bSKI)
	g.svc.Start()
	defer g.svc.Shutdown()

	select {
	case err := <-g.written:
		if err != nil {
			t.Fatalf("limit write: %v", err)
		}
	case <-time.After(60 * time.Second):
		t.Fatal("no SHIP connection within 60 s (is mDNS available?)")
	}

	deadline := time.After(15 * time.Second)
	for {
		select {
		case o := <-lines:
			if o.Active && o.LimitW != nil && *o.LimitW == 7000 {
				t.Logf("gateway received: active, %.0f W, failsafe %v", *o.LimitW, o.Failsafe)
				return
			}
		case <-deadline:
			t.Fatal("the limit did not reach the gateway")
		}
	}
}
