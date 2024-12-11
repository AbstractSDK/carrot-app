FROM prom/prometheus:v2.55.1

# copy the Prometheus configuration file
COPY prometheus.yml /etc/prometheus/prometheus.yml

# expose the Prometheus server port
EXPOSE 9090

# set the entrypoint command
USER root
ENTRYPOINT [ "/bin/prometheus" ]
CMD        [ "--config.file=/etc/prometheus/prometheus.yml", \
             "--storage.tsdb.path=/prometheus", \
             "--storage.tsdb.retention=365d", \
             "--web.console.libraries=/usr/share/prometheus/console_libraries", \
             "--web.console.templates=/usr/share/prometheus/consoles", \
             "--web.external-url=http://prometheus.abstract.money", \
             "--log.level=info"]
 
