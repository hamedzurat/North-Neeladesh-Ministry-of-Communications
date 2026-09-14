# Faction Relationships

Arrows show how one faction currently treats another. These relationships can change during the game.

```mermaid
flowchart LR
    NRP["National Reconstruction Party"]
    SPD["State Protection Directorate"]
    ARMY["National Army Command"]
    WORKERS["Red Earth Workers' Congress"]
    AUTONOMY["Riverland Autonomy League"]
    TNB["True North Brigade"]
    SOUTH["Government of South Neeladesh"]

    NRP -->|"supports and uses"| SPD
    SPD -->|"protects for now"| NRP
    NRP -->|"secretly uses"| TNB
    TNB -->|"supports the president"| NRP

    NRP -.->|"hates"| WORKERS
    NRP -.->|"hates"| AUTONOMY

    SPD -.->|"hates"| WORKERS
    SPD -.->|"hates"| AUTONOMY
    SPD -.->|"rivals"| ARMY

    ARMY -.->|"rivals"| SPD
    ARMY -.->|"distrusts"| TNB
    ARMY -.->|"opposes interference"| SOUTH

    WORKERS -->|"cooperates with"| AUTONOMY
    WORKERS -.->|"hates"| NRP
    WORKERS -.->|"distrusts"| SOUTH

    AUTONOMY -->|"cooperates with"| WORKERS
    AUTONOMY -.->|"hates"| TNB
    AUTONOMY -.->|"hates"| SPD
    AUTONOMY -.->|"distrusts"| SOUTH

    TNB -.->|"hates"| AUTONOMY
    TNB -.->|"hates"| WORKERS
    TNB -.->|"hates"| SOUTH

    SOUTH -->|"tries to use"| AUTONOMY
```
