# Disposable OpenSSH server used by the SSH integration tests and manual testing.
# Contains the tools needed by the phase 2 checks (vim, htop, less, ping).
FROM alpine:3.22
RUN apk add --no-cache openssh-server bash vim htop less iputils-ping ncurses \
 && ssh-keygen -A \
 && adduser -D -u 1000 -s /bin/bash netforge \
 && echo 'netforge:netforge-test' | chpasswd \
 && install -d -m 700 -o netforge -g netforge /home/netforge/.ssh \
 && sed -i 's/^#\?PasswordAuthentication.*/PasswordAuthentication yes/' /etc/ssh/sshd_config \
 && seq 1 5000 | sed 's/^/ligne numéro /' > /home/netforge/big.txt
COPY sshd_legacy_config /etc/ssh/sshd_legacy_config
EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]
