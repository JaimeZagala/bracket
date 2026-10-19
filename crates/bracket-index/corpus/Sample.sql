-- Synthetic T-SQL for the symbol tests.
-- CREATE PROCEDURE NotThisOne (a comment)
CREATE TABLE [dbo].[Invoice] (
    [Id] INT NOT NULL PRIMARY KEY,
    [Number] NVARCHAR(20) NOT NULL
);
GO

ALTER TABLE dbo.Invoice ADD [Total] DECIMAL(18, 2) NULL;
GO

CREATE PROCEDURE [dbo].[GetInvoice]
    @Id INT
AS
BEGIN
    SELECT * FROM dbo.Invoice WHERE Id = @Id;
END
GO

create or alter function dbo.InvoiceTotal(@Id int) returns decimal(18, 2)
as begin return 0 end
go

ALTER VIEW Reporting.OpenInvoices AS SELECT Id FROM dbo.Invoice;
GO
CREATE TRIGGER trg_Invoice_Insert ON dbo.Invoice AFTER INSERT AS SELECT 1;
GO
CREATE TYPE dbo.InvoiceIds AS TABLE (Id INT);
