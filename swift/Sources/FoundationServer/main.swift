import Vapor

EnvBootstrap.loadDotEnvIfPresent()

let environment = try Environment.detect()
let app = try await Application.make(environment)

do {
    try configure(app)
    try await app.execute()
    try await app.asyncShutdown()
} catch {
    try? await app.asyncShutdown()
    throw error
}
