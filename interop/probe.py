import c104, time
client = c104.Client(tick_rate_ms=50, command_timeout_ms=3000)
conn = client.add_connection(ip="127.0.0.1", port=2404, init=c104.Init.ALL)
st = conn.add_station(common_address=1)
got = []
def on_new_point(client: c104.Client, station: c104.Station, io_address: int, point_type: c104.Type) -> None:
    got.append((io_address, str(point_type)))
    station.add_point(io_address=io_address, type=point_type)
client.on_new_point(callable=on_new_point)
cmd = st.add_point(io_address=5001, type=c104.Type.C_SC_NA_1)
lim = st.add_point(io_address=5002, type=c104.Type.C_SE_NC_1)
client.start()
for _ in range(50):
    if conn.is_connected: break
    time.sleep(0.1)
print("connected", conn.is_connected, conn.state)
time.sleep(1.5)
print("points after GI:", sorted(got))
for p in st.points:
    print(p.io_address, p.type, p.value, p.quality)
cmd.value = True
print("dim ON ->", cmd.transmit(cause=c104.Cot.ACTIVATION))
lim.value = 60.0
print("limit 60 ->", lim.transmit(cause=c104.Cot.ACTIVATION))
lim.value = 150.0
print("limit 150 (invalid) ->", lim.transmit(cause=c104.Cot.ACTIVATION))
time.sleep(3)
for p in st.points:
    if p.io_address in (1001,1004,1005,1006,2001):
        print(p.io_address, p.type, round(p.value,2) if isinstance(p.value,float) else p.value)
cmd.value = False
print("dim OFF ->", cmd.transmit(cause=c104.Cot.ACTIVATION))
lim.value = 100.0
print("limit 100 ->", lim.transmit(cause=c104.Cot.ACTIVATION))
client.stop()
