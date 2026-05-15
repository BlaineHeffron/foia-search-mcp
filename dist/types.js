export class SourceError extends Error {
    source;
    status;
    action;
    constructor(message, source, status, action) {
        super(message);
        this.source = source;
        this.status = status;
        this.action = action;
        this.name = "SourceError";
    }
}
