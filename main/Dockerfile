FROM swift:6.0-jammy

WORKDIR /app

COPY Package.swift Package.resolved ./
RUN swift package resolve

COPY Sources ./Sources
RUN swift build -c release --product FoundationServer

EXPOSE 8000

CMD [".build/release/FoundationServer", "serve", "--env", "production", "--hostname", "0.0.0.0", "--port", "8000"]
