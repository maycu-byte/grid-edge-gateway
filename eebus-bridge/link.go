package main

import (
	"encoding/json"
	"log"
	"net"
	"time"
)

// Resend at least this often: the gateway treats silence as a lost bridge.
const keepAlive = 5 * time.Second

// Link keeps a TCP connection to the gateway and sends it the output of
// the state whenever it changes, and at least every keepAlive.
type Link struct {
	addr string
	conn net.Conn
	last []byte
	sent time.Time
}

func NewLink(addr string) *Link { return &Link{addr: addr} }

func (l *Link) Send(o Output, now time.Time) {
	line, err := json.Marshal(o)
	if err != nil {
		log.Printf("gateway link: %v", err)
		return
	}
	line = append(line, '\n')
	if l.conn != nil && string(line) == string(l.last) && now.Sub(l.sent) < keepAlive {
		return
	}
	if l.conn == nil {
		c, err := net.DialTimeout("tcp", l.addr, 2*time.Second)
		if err != nil {
			log.Printf("gateway link: %v", err)
			return
		}
		log.Printf("gateway link: connected to %s", l.addr)
		l.conn = c
	}
	_ = l.conn.SetWriteDeadline(now.Add(2 * time.Second))
	if _, err := l.conn.Write(line); err != nil {
		log.Printf("gateway link: %v", err)
		_ = l.conn.Close()
		l.conn = nil
		return
	}
	if string(line) != string(l.last) {
		log.Printf("to gateway: %s", line[:len(line)-1])
	}
	l.last, l.sent = line, now
}
