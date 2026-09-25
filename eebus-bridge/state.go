package main

import "time"

// InitTimeout is how long a Controllable System may go without the energy
// guard's first heartbeat before it enters the failsafe state (LPC, "init").
// The heartbeat itself counts as lost after 2 minutes (eebus-go checks that).
const InitTimeout = 120 * time.Second

// Output is what the bridge passes to the gateway: one JSON line.
type Output struct {
	Active         bool     `json:"active"`
	LimitW         *float64 `json:"limit_w"`
	Failsafe       bool     `json:"failsafe"`
	FailsafeLimitW *float64 `json:"failsafe_limit_w"`
}

// State follows the LPC use case on the Controllable System side:
//
//   - a limit written by the energy guard applies while it is active and its
//     duration has not run out;
//   - without the guard's heartbeat (none in the first 2 minutes, or lost
//     later) the failsafe limit applies;
//   - the failsafe state ends only with the heartbeat back and either a new
//     limit from the guard or the failsafe minimum duration elapsed.
type State struct {
	started time.Time

	limitActive bool
	limitW      float64
	limitUntil  time.Time // zero: no end
	limitAt     time.Time // when the last limit was written

	failsafeW   float64
	failsafeMin time.Duration

	heartbeatSeen bool
	heartbeatOK   bool

	inFailsafe    bool
	failsafeSince time.Time
}

func NewState(now time.Time, failsafeW float64, failsafeMin time.Duration) *State {
	return &State{started: now, failsafeW: failsafeW, failsafeMin: failsafeMin}
}

// SetLimit records a limit the energy guard wrote. A duration of 0 has no end.
func (s *State) SetLimit(active bool, valueW float64, duration time.Duration, now time.Time) {
	s.limitActive = active
	s.limitW = valueW
	s.limitAt = now
	s.limitUntil = time.Time{}
	if duration > 0 {
		s.limitUntil = now.Add(duration)
	}
}

// SetFailsafe records the failsafe limit and minimum duration.
func (s *State) SetFailsafe(valueW float64, minimum time.Duration) {
	s.failsafeW = valueW
	s.failsafeMin = minimum
}

// Heartbeat records whether the guard's heartbeat is within its window.
func (s *State) Heartbeat(ok bool, now time.Time) {
	if ok {
		s.heartbeatSeen = true
	}
	s.heartbeatOK = ok
	s.update(now)
}

func (s *State) update(now time.Time) {
	lost := (s.heartbeatSeen && !s.heartbeatOK) || (!s.heartbeatSeen && now.Sub(s.started) >= InitTimeout)
	if !s.inFailsafe {
		if lost {
			s.inFailsafe = true
			s.failsafeSince = now
		}
		return
	}
	newLimit := s.limitAt.After(s.failsafeSince)
	if s.heartbeatOK && (newLimit || now.Sub(s.failsafeSince) >= s.failsafeMin) {
		s.inFailsafe = false
	}
}

// Output is the state the gateway has to apply now.
func (s *State) Output(now time.Time) Output {
	s.update(now)
	fs := s.failsafeW
	out := Output{FailsafeLimitW: &fs}
	if s.inFailsafe {
		out.Active, out.Failsafe, out.LimitW = true, true, &fs
		return out
	}
	if s.limitActive && (s.limitUntil.IsZero() || now.Before(s.limitUntil)) {
		w := s.limitW
		out.Active, out.LimitW = true, &w
	}
	return out
}
