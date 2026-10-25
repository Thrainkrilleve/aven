# Claude Opus 5.5 Benchmark Review & Operational Audit

**Model:** Claude Opus 5.5 (Medium Thinking)  
**Date:** October 24, 2026  
**Focus:** Operational Failure Modes, Queueing Theory Limits, Systemd Lifecycle, and Physical Network Overheads

---

## 1. Critical Problems Found in Inputs & Operational Assumptions

1. **The compiled program isn’t a server:**
   `server_capacity.c` prints three numbers and exits. Prompts 1 and 4 ask for a long-running worker service with a Restart policy. With `Restart=always`, systemd would keep restarting it until it hit the start limit. For this binary, the unit should be `Type=oneshot` with `RemainAfterExit=yes`.
2. **25.6 Gbps doesn’t fit on a 25 GbE NIC:**
   That figure is payload only, counting 1 KB as 1000 bytes. Counted as KiB, it’s 26.2 Gbps, and TCP/IP/Ethernet overhead adds roughly another 3–5%. You need 40/50/100 GbE or bonded links.
3. **1,000 workers leaves no headroom:**
   By Little’s law, $50,000\text{ req/s} \times 0.020\text{ s} = 1,000\text{ workers}$, which is correct but means 100% utilization, where queues grow without bound under stochastic traffic. To run at 70%, use about 1,430 workers, or about 28.6 GB RAM at 20 MB each. The 20 GB figure also leaves out the OS and kernel socket memory.
4. **18 drives give 103.68 TB, not 100 TB:**
   The storage solve gives 17.36 drives. You can’t buy a fraction of a drive, so you need 18.
5. **RAID 6 isn’t fixed at 0.75 efficiency:**
   RAID 6 efficiency is $(n-2)/n$, so 0.75 only holds for 8-drive (6+2) groups. One 18-drive RAID 6 is 0.889. Ceph usually doesn’t use RAID at all: erasure coding 6+2 gives 0.75, and 3× replication gives 0.333.
6. **Prompt 3 asks to “pin all 6 unknowns”:**
   You only need 3 more values. Setting all 6 on top of the 2 targets over-determines the system.
7. **Minor:**
   The service is called `aven-worker.service` in Prompt 1 and `aven-server.service` in Prompt 4.

---

## 2. Low-Level Verification

* **C output:** `total_ram_needed = 20000`, `reqs_per_second = 50000`, `bandwidth_gbps = 25.6`. These match the model.
* **Hardened build:** Compiled with Prompt 4 flags, `readelf` shows PIE, GNU_RELRO plus BIND_NOW (Full RELRO), a non-exec stack, and `printf` $\to$ `__printf_chk` (fortify is active).
* **No stack canary in binary:** `-fstack-protector-strong` only instruments functions that need it. After inlining, nothing does, so `checksec` will say “No canary found”. That’s expected, not a misconfigured build.
* **Divide by zero:** `avg_latency_ms = 0.0` gives `inf`. If `workers = 0` as well, it’s `0/0`, which printed `-nan`. Neither crashes: IEEE-754 doubles don’t trap by default.

---

## 3. Recommended Configurations

### /etc/sysctl.d/99-aven.conf:
```conf
net.core.somaxconn = 65535
net.ipv4.tcp_max_syn_backlog = 65535
net.core.netdev_max_backlog = 250000
net.core.rmem_max = 67108864
net.core.wmem_max = 67108864
net.ipv4.tcp_rmem = 4096 131072 67108864
net.ipv4.tcp_wmem = 4096 65536 67108864
# fs.file-max: run `sysctl fs.file-max` first. RHEL 9 is usually already very large,
# and hard-coding something like 2097152 can LOWER it. Only set it if it's below ~2M.
```

### /etc/systemd/system/aven-worker.service (for a real long-running worker):
```ini
[Unit]
Description=Aven worker
Wants=network-online.target
After=network-online.target
StartLimitIntervalSec=60
StartLimitBurst=5

[Service]
Type=exec
User=aven
Group=aven
ExecStart=/opt/aven/bin/aven-worker
Restart=on-failure
RestartSec=2s
LimitNOFILE=1048576
TasksMax=4096
CapabilityBoundingSet=CAP_NET_BIND_SERVICE
AmbientCapabilities=CAP_NET_BIND_SERVICE
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
RestrictNamespaces=true
LockPersonality=true
MemoryDenyWriteExecute=true
SystemCallArchitectures=native
StateDirectory=aven
LogsDirectory=aven

[Install]
WantedBy=multi-user.target
```

### Production Ansible Playbook (deploy_aven_node.yml):
```yaml
---
- name: Deploy Aven capacity node
  hosts: aven_nodes
  become: true
  vars:
    aven_root: /opt/aven
    aven_cflags: >-
      -std=c99 -O2 -Wall -Werror=format-security
      -fstack-protector-strong -fstack-clash-protection -fcf-protection
      -D_FORTIFY_SOURCE=2 -fPIE -pie -Wl,-z,relro,-z,now
  tasks:
    - name: Install build and SELinux tooling
      ansible.builtin.dnf:
        name: [gcc, make, glibc-devel, policycoreutils-python-utils]
        state: present

    - name: Apply kernel tuning
      ansible.builtin.copy:
        dest: /etc/sysctl.d/99-aven.conf
        content: |
          net.core.somaxconn = 65535
          net.ipv4.tcp_max_syn_backlog = 65535
        owner: root
        group: root
        mode: "0644"
      notify: Reload sysctl

    - name: Create aven system user
      ansible.builtin.user:
        name: aven
        system: true
        shell: /sbin/nologin
        create_home: false

    - name: Create directories
      ansible.builtin.file:
        path: "{{ item }}"
        state: directory
        owner: root
        group: root
        mode: "0755"
      loop: ["{{ aven_root }}", "{{ aven_root }}/bin"]

    - name: Copy source
      ansible.builtin.copy:
        src: files/server_capacity.c
        dest: "{{ aven_root }}/server_capacity.c"
        mode: "0644"
      register: aven_src

    - name: Check for existing binary
      ansible.builtin.stat:
        path: "{{ aven_root }}/bin/server_capacity"
      register: aven_bin

    - name: Compile hardened binary
      ansible.builtin.command:
        cmd: "gcc {{ aven_cflags }} -o {{ aven_root }}/bin/server_capacity {{ aven_root }}/server_capacity.c"
      when: aven_src.changed or not aven_bin.stat.exists
      notify: Restart aven-server

    - name: Register SELinux file context
      community.general.sefcontext:
        target: "{{ aven_root }}/bin(/.*)?"
        setype: bin_t
        state: present

    - name: Restore SELinux contexts
      ansible.builtin.command: restorecon -Rv {{ aven_root }}
      register: aven_restorecon
      changed_when: aven_restorecon.stdout | length > 0

    - name: Install systemd unit
      ansible.builtin.copy:
        dest: /etc/systemd/system/aven-server.service
        content: |
          [Unit]
          Description=Aven capacity report

          [Service]
          Type=oneshot
          RemainAfterExit=yes
          User=aven
          ExecStart={{ aven_root }}/bin/server_capacity
          NoNewPrivileges=true
          ProtectSystem=strict
          ProtectHome=true
          PrivateTmp=true

          [Install]
          WantedBy=multi-user.target
        owner: root
        group: root
        mode: "0644"
      notify: Restart aven-server

    - name: Enable and start aven-server
      ansible.builtin.systemd:
        name: aven-server
        enabled: true
        state: started
        daemon_reload: true

  handlers:
    - name: Reload sysctl
      ansible.builtin.command: sysctl --system

    - name: Restart aven-server
      ansible.builtin.systemd:
        name: aven-server
        state: restarted
        daemon_reload: true
```
