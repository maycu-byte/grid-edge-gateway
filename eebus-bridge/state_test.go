package main

import (
	"testing"
	"time"
)

var t0 = time.Date(2026, 1, 20, 17, 0, 0, 0, time.UTC)

func at(d time.Duration) time.Time { return t0.Add(d) }

func limit(o Output) float64 {
	if o.LimitW == nil {
		return -1
	}
	return *o.LimitW
}

func TestALimitAppliesUntilItsDurationRunsOut(t *testing.T) {
	s := NewState(t0, 4200, 2*time.Hour)
	s.Heartbeat(true, at(0))
	if o := s.Output(at(0)); o.Active {
		t.Fatalf("no limit yet: %+v", o)
	}
	s.SetLimit(true, 11000, 2*time.Hour, at(time.Minute))
	if o := s.Output(at(time.Hour)); !o.Active || limit(o) != 11000 || o.Failsafe {
		t.Fatalf("limit in force: %+v", o)
	}
	if o := s.Output(at(2*time.Hour + 2*time.Minute)); o.Active {
		t.Fatalf("duration over: %+v", o)
	}
}

func TestADeactivatedLimitLiftsTheReduction(t *testing.T) {
	s := NewState(t0, 4200, 2*time.Hour)
	s.Heartbeat(true, at(0))
	s.SetLimit(true, 4200, 0, at(0))
	s.SetLimit(false, 4200, 0, at(time.Minute))
	if o := s.Output(at(2 * time.Minute)); o.Active {
		t.Fatalf("limit off: %+v", o)
	}
}

func TestWithoutAFirstHeartbeatTheFailsafeLimitAppliesAfterTwoMinutes(t *testing.T) {
	s := NewState(t0, 6000, 2*time.Hour)
	s.Heartbeat(false, at(time.Minute))
	if o := s.Output(at(time.Minute)); o.Active {
		t.Fatalf("still in init: %+v", o)
	}
	s.Heartbeat(false, at(InitTimeout))
	if o := s.Output(at(InitTimeout)); !o.Active || !o.Failsafe || limit(o) != 6000 {
		t.Fatalf("failsafe expected: %+v", o)
	}
}

func TestTheFailsafeStateEndsWithTheHeartbeatAndANewLimit(t *testing.T) {
	s := NewState(t0, 4200, 2*time.Hour)
	s.Heartbeat(true, at(0))
	s.Heartbeat(false, at(10*time.Minute))
	if o := s.Output(at(10 * time.Minute)); !o.Failsafe {
		t.Fatalf("heartbeat lost: %+v", o)
	}
	// The heartbeat alone is not enough before the minimum duration.
	s.Heartbeat(true, at(20*time.Minute))
	if o := s.Output(at(20 * time.Minute)); !o.Failsafe {
		t.Fatalf("still failsafe: %+v", o)
	}
	s.SetLimit(false, 0, 0, at(21*time.Minute))
	s.Heartbeat(true, at(21*time.Minute))
	if o := s.Output(at(21 * time.Minute)); o.Failsafe || o.Active {
		t.Fatalf("new limit written: %+v", o)
	}
}

func TestTheFailsafeStateEndsAfterItsMinimumDuration(t *testing.T) {
	s := NewState(t0, 4200, 2*time.Hour)
	s.Heartbeat(true, at(0))
	s.Heartbeat(false, at(time.Minute))
	s.Heartbeat(true, at(time.Hour))
	if o := s.Output(at(time.Hour)); !o.Failsafe {
		t.Fatalf("before the minimum: %+v", o)
	}
	s.Heartbeat(true, at(2*time.Hour+time.Minute))
	if o := s.Output(at(2*time.Hour + time.Minute)); o.Failsafe {
		t.Fatalf("minimum over: %+v", o)
	}
}
